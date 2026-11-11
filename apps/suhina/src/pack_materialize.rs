// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped pack unpacking into the library ([issue 1279] › the library): the "Suhina"
//! pack's files ride inside the executable, and each one lands in its device's library folder
//! (`Sounds/NAM/Amp models/Suhina/…`, `Sounds/NAM/Cabinets/Suhina/…`) when it is not there
//! already. A file the user carries is theirs: it is never overwritten, never moved. Pack
//! write failures stay failures wholly on the page: the pack still plays from the executable,
//! and the next start can try again.

use std::path::{Path, PathBuf};

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
/// ([docs/spec/nam-amp-and-cab.md] › the unpack). The write is the session's
/// staging-and-rename ([ADR 0036] › 5, the review). A file at the target is never written
/// over: the check before the copy is the promise — a file that appeared while the copy ran
/// is another start's, and its bytes are the pack's own.
fn publish(target: &Path, bytes: &[u8]) -> bool {
    if target.exists() {
        return false;
    }
    let staging =
        target.with_file_name(format!("{}.suhina-new", target.file_name().unwrap_or_default().to_string_lossy()));
    suhina_session::stage_and_rename(&staging, target, bytes).is_ok()
}

/// The old default folders each device's files may still carry
/// ([issue 1279] · the library move): `(device id, old folder name)`.
const OLD_DEVICE_FOLDERS: &[(&str, &str)] = &[("nam-amp-modeler", "NAM models"), ("nam-cab-loader", "IRs")];
/// The marker that says an old folder's kept files were reported: the warning is a start-up
/// record, not a per-start nag, and the folder sits quiet about them after until something in
/// it moves ([#1290] › the review). It rides beside the folder — inside it, the move's walk
/// would carry it into the new tree and the emptied-folder removal would refuse it — and is
/// removed when the folder goes.
fn migrate_reported(base: &Path, old_name: &str) -> PathBuf {
    base.join(format!(".suhina-migrate-{}-reported", old_name.to_lowercase().replace(' ', "-")))
}

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
            let _ = std::fs::remove_file(migrate_reported(base, old_name));
            if debug {
                eprintln!("pack migrate: removed the emptied old folder {old:?}");
            }
        }
        // The kept files' names are in the new tree and stay put: the folder never empties, so
        // the same start-up warning would come again at every start. The move records that it
        // was reported and sits quiet about them after ([#1290] › the review) — a start that
        // moves or cannot move something still speaks, so a conflict the user resolves or a
        // permission that heals is never silent.
        let reported = migrate_reported(base, old_name);
        let reported_before = reported.exists();
        match (moved, kept, stuck) {
            (0, 0, 0) => {}
            (moved, 0, 0) => warnings.push(format!(
                "your old {} folder ({}) moved into the library's {} folder: {moved} file(s)",
                old_name,
                old.display(),
                library_folder
            )),
            (0, kept, 0) if !reported_before => warnings.push(format!(
                "your old {} folder ({}) kept its {kept} file(s): their names are already in the library's {} folder",
                old_name,
                old.display(),
                library_folder
            )),
            (0, _kept, 0) => {}
            (0, 0, stuck) => warnings.push(format!(
                "your old {} folder ({}) could not move its {stuck} file(s) into the library's {} folder (a permission or \
                 the filesystem refused); they stay at {}",
                old_name,
                old.display(),
                library_folder,
                old.display()
            )),
            (moved, kept, 0) if moved > 0 => warnings.push(format!(
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
        if kept > 0 && !reported_before {
            let _ = std::fs::write(&reported, b"the kept files were reported\n");
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
#[path = "pack_materialize_tests.rs"]
mod tests;
