// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped pack unpacking into the library ([issue 1279] › the library): the "Suhina"
//! pack's files ride inside the executable, and each one lands in its device's library folder
//! (`Sounds/NAM/Amp models/Suhina/…`, `Sounds/NAM/Cabinets/Suhina/…`) when it is not there
//! already. A file the user carries is theirs: it is never overwritten, never moved. Pack
//! write failures stay failures wholly on the page: the pack still plays from the executable,
//! and the next start can try again.

use std::path::Path;

/// Writes every pack file when its target is missing, under the library's NAM `sounds`
/// folder: `<device folder>/<pack folder>/<file name>` — the pack's own `Suhina` folder inside
/// each device's library folder ([ADR 0036] › the library). Returns how many files were
/// written; the count is for a start-up log at most. `SUHINA_PACK_DEBUG=1` prints the walk to
/// stderr.
pub fn unpack_pack(sounds: &Path) -> usize {
    let debug = std::env::var("SUHINA_PACK_DEBUG").as_deref() == Ok("1");
    if debug {
        eprintln!("pack unpack from {sounds:?} with {} rows", suhina_device_registry::pack_files_flat().len());
    }
    let mut written = 0;
    for (device, library_folder, pack_folder, name, bytes) in suhina_device_registry::pack_files_flat() {
        let pack_over = sounds.join(library_folder).join(pack_folder);
        let Some(file_extension) = suhina_device_registry::file_extension(device) else { continue };
        let target = pack_over.join(name);
        if target.exists() {
            if debug {
                eprintln!("pack unpack: {name} is already carried in {pack_over:?}, skipping");
            }
            continue;
        }
        if !name.ends_with(&format!(".{file_extension}")) {
            if debug {
                eprintln!("pack unpack: {name} does not end in .{file_extension}, skipping");
            }
            continue;
        }
        if std::fs::create_dir_all(&pack_over).is_err() {
            if debug {
                eprintln!("pack unpack: {pack_over:?} could not be made, skipping");
            }
            continue;
        }
        if publish(&target, bytes) {
            if debug {
                eprintln!("pack unpack: wrote {} bytes to {target:?}", bytes.len());
            }
            written += 1;
        } else if debug {
            eprintln!("pack unpack: {target:?} did not write; the pack stays in the executable");
        }
    }
    // The pack's provenance and licence records go beside the files: the README that says
    // where every file came from and under what licence, and the licence texts the collected
    // IRs were published under — a redistribution keeps them beside the files
    // ([ADR 0036] › 6). A record the user carries is never overwritten, like a pack file's.
    for (library_folder, pack_folder, name, bytes) in suhina_device_registry::pack_records() {
        let pack_over = sounds.join(library_folder).join(pack_folder);
        let target = pack_over.join(name);
        if target.exists() {
            if debug {
                eprintln!("pack unpack: {name} is already carried in {pack_over:?}, skipping");
            }
            continue;
        }
        if std::fs::create_dir_all(&pack_over).is_err() {
            if debug {
                eprintln!("pack unpack: {pack_over:?} could not be made, skipping");
            }
            continue;
        }
        if publish(&target, bytes) {
            if debug {
                eprintln!("pack unpack: wrote {} bytes to {target:?}", bytes.len());
            }
            written += 1;
        } else if debug {
            eprintln!("pack unpack: {target:?} did not write; the record stays in the executable");
        }
    }
    if debug {
        eprintln!("pack unpack: {written} file(s) written");
    }
    written
}

/// Writes `bytes` to `target` beside it first, then renames the completed file into place —
/// a write that fails (or an app that ends mid-write) leaves no partial file at the target,
/// and the rename publishes the completed bytes only
/// ([docs/spec/nam-amp-and-cab.md] › the unpack). A file that appeared at the target while
/// the copy ran (the user's own, or another start's) is never written over. Returns whether
/// it published.
fn publish(target: &Path, bytes: &[u8]) -> bool {
    let staging =
        target.with_file_name(format!("{}.suhina-new", target.file_name().unwrap_or_default().to_string_lossy()));
    if std::fs::write(&staging, bytes).is_err() {
        let _ = std::fs::remove_file(&staging);
        return false;
    }
    if target.exists() {
        let _ = std::fs::remove_file(&staging);
        return false;
    }
    match std::fs::rename(&staging, target) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&staging);
            false
        }
    }
}

/// The old default folders each device's files may still carry
/// ([issue 1279] · the library move): `(device id, old folder name)`.
const OLD_DEVICE_FOLDERS: &[(&str, &str)] = &[("nam-amp-modeler", "NAM models"), ("nam-cab-loader", "IRs")];

/// Moves the old defaults' contents into the library's device folders before the pack unpacks:
/// `base` is the library's root (`Documents/Suhina`), `sounds` the NAM sounds under it — the
/// old folders sit at the root, the device folders under `sounds`. A pack subfolder moves as a
/// pack, a file moves by its name, and a name the new tree already holds stays where it is, so
/// nothing is ever overwritten ([issue 1279]). Each moved-empty old folder is removed; the
/// returned lines go to the start-up warnings.
pub fn migrate_old_folders(base: &Path, sounds: &Path) -> Vec<String> {
    let debug = std::env::var("SUHINA_PACK_DEBUG").as_deref() == Ok("1");
    let mut warnings = Vec::new();
    for (device, old_name) in OLD_DEVICE_FOLDERS {
        let Some(library_folder) = suhina_device_registry::pack_files_flat()
            .iter()
            .find_map(|(id, folder, _, _, _)| (*id == *device).then_some(*folder))
        else {
            continue;
        };
        let old = base.join(old_name);
        let fresh = sounds.join(library_folder);
        if !old.is_dir() {
            if debug {
                eprintln!("pack migrate: no old {old_name} folder at {old:?}");
            }
            continue;
        }
        if debug {
            eprintln!("pack migrate: moving {old:?} into {fresh:?}");
        }
        let _ = std::fs::create_dir_all(&fresh);
        let mut moved = 0;
        let mut kept = 0;
        let mut stuck = 0;
        let entries = std::fs::read_dir(&old).map(|entries| entries.flatten().collect::<Vec<_>>()).unwrap_or_default();
        for entry in entries {
            let target = fresh.join(entry.file_name());
            let source = entry.path();
            let (entry_moved, entry_kept, entry_stuck) = move_entry(&source, &target);
            if entry_moved > 0 {
                if debug {
                    eprintln!("pack migrate: moved {:?}", entry.file_name());
                }
                moved += entry_moved;
            }
            if entry_kept > 0 {
                if debug {
                    eprintln!("pack migrate: kept {:?} where it is (its name is in the new tree)", entry.file_name());
                }
                kept += entry_kept;
            }
            if entry_stuck > 0 {
                if debug {
                    eprintln!("pack migrate: could not move {:?}", entry.file_name());
                }
                stuck += entry_stuck;
            }
        }
        if old.is_dir() && is_empty(&old) {
            let _ = std::fs::remove_dir(&old);
            if debug {
                eprintln!("pack migrate: removed the emptied old folder {old:?}");
            }
        }
        match (moved, kept, stuck) {
            (0, 0, 0) => {}
            (moved, 0, 0) => warnings.push(format!(
                "your old {} folder ({}) moved into the library's {} folder: {moved} file(s)",
                old_name,
                old.display(),
                library_folder
            )),
            (0, kept, 0) => warnings.push(format!(
                "your old {} folder ({}) kept its {kept} file(s): their names are already in the library's {} folder",
                old_name,
                old.display(),
                library_folder
            )),
            (0, 0, stuck) => warnings.push(format!(
                "your old {} folder ({}) could not move its {stuck} file(s) into the library's {} folder (a permission or \
                 the filesystem refused); they stay at {}",
                old_name,
                old.display(),
                library_folder,
                old.display()
            )),
            (moved, kept, 0) => warnings.push(format!(
                "your old {} folder ({}) mostly moved into the library's {} folder: {moved} file(s), {kept} kept in place \
                 because their names were already there",
                old_name,
                old.display(),
                library_folder
            )),
            (moved, kept, stuck) => warnings.push(format!(
                "your old {} folder ({}) mostly moved into the library's {} folder: {moved} file(s), {kept} kept in place \
                 because their names were already there, {stuck} that could not move and stay at {}",
                old_name,
                old.display(),
                library_folder,
                old.display()
            )),
        }
    }
    warnings
}

/// Moves one file or folder into `target` when it is missing there. Folders move recursively,
/// their own names carried; a folder that lands only in part stays whole beside its moved
/// files. A `target` that exists is never written over. Returns `(moved, kept, stuck)` — the
/// files that landed in the new tree, the ones kept in place because the new tree holds their
/// name, and the ones that could not move at all (a permission or the filesystem refused the
/// rename) — so a folder that lands only in part is reported as the partial move it is, and a
/// failed move is never silent: the file stays in the old folder and the user hears why.
/// Every source is classified by its own kind (`symlink_metadata`), never by following a
/// link: a symlink in the user's old folder renames as the one entry it is and nothing reads
/// through it, whatever it points at — a link to an ancestor can neither loop the walk nor
/// drag the link target's real files into the new tree's nested paths. A link counts no
/// files: the link itself moves, not content.
fn move_entry(source: &Path, target: &Path) -> (usize, usize, usize) {
    let kind = match std::fs::symlink_metadata(source) {
        Ok(kind) => kind,
        // The entry went away between the listing and the move: nothing there to count.
        Err(_) => return (0, 0, 0),
    };
    // The name the new tree holds — by its own kind too, a dangling link included — is never
    // written over.
    if std::fs::symlink_metadata(target).is_ok() {
        if !kind.is_dir() {
            return (0, 1, 0);
        }
        let (mut moved, mut kept, mut stuck) = (0, 0, 0);
        let entries =
            std::fs::read_dir(source).map(|entries| entries.flatten().collect::<Vec<_>>()).unwrap_or_default();
        for entry in entries {
            let (inner_moved, inner_kept, inner_stuck) = move_entry(&entry.path(), &target.join(entry.file_name()));
            moved += inner_moved;
            kept += inner_kept;
            stuck += inner_stuck;
        }
        if is_empty(source) {
            let _ = std::fs::remove_dir(source);
        }
        return (moved, kept, stuck);
    }
    // The whole entry renames where its name is missing: a folder's files are the move the
    // warning reports, a file is one, a symlink is none. A refused rename (a permission, a
    // different filesystem) leaves the entry where it is — that is neither a move nor a kept
    // name, and it gets its own count.
    let count = if kind.is_file() {
        1
    } else if kind.is_dir() {
        files_in(source)
    } else {
        0
    };
    if std::fs::rename(source, target).is_ok() { (count, 0, 0) } else { (0, 0, count) }
}

/// Counts the files inside `folder`, its subfolders included, so a whole folder's move
/// reports the files it carried. The walk does not follow symlinks (`DirEntry::file_type`
/// reads the entry's own kind): a link to an ancestor in the user's old folder would make
/// `is_dir` recursion run forever and overflow the stack at start-up.
fn files_in(folder: &Path) -> usize {
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| match entry.file_type() {
                    Ok(kind) if kind.is_dir() => files_in(&entry.path()),
                    Ok(kind) if kind.is_file() => 1,
                    _ => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

fn is_empty(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_none())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(
            std::fs::read(new_pack.join("loop")).unwrap(),
            b"the user's own link-name file",
            "the link's name kept"
        );
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
}
