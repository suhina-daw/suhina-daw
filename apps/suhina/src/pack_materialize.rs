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
        match std::fs::write(&target, bytes) {
            Ok(()) => {
                if debug {
                    eprintln!("pack unpack: wrote {} bytes to {target:?}", bytes.len());
                }
                written += 1;
            }
            Err(error) => {
                if debug {
                    eprintln!("pack unpack: {target:?} did not write ({error:?}); the pack stays in the executable");
                }
            }
        }
    }
    if debug {
        eprintln!("pack unpack: {written} file(s) written");
    }
    written
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
        let again = unpack_pack(&dir);
        assert_eq!(again, 0, "a second start overwrote or rewrote the user's files");
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
