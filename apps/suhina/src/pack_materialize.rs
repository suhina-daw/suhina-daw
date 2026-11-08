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
/// folder. Returns how many files were written; the count is for a start-up log at most.
pub fn unpack_pack(sounds: &Path) -> usize {
    let mut written = 0;
    for (device, folder, name, bytes) in suhina_device_registry::pack_files_flat() {
        let device_folder = sounds.join(folder);
        let Some(file_extension) = suhina_device_registry::file_extension(device) else { continue };
        let target = device_folder.join(name);
        if target.exists() {
            continue;
        }
        if !name.ends_with(&format!(".{file_extension}")) {
            continue;
        }
        if std::fs::create_dir_all(&device_folder).is_err() {
            continue;
        }
        if std::fs::write(&target, bytes).is_ok() {
            written += 1;
        }
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
        for (device, folder, name, bytes) in suhina_device_registry::pack_files_flat() {
            let target = dir.join(folder).join(name);
            assert!(target.is_file(), "{target:?} is not there after unpack_pack");
            assert_eq!(
                std::fs::read(&target).unwrap(),
                bytes,
                "{name}'s copy is not the pack's bytes (device {device:?})"
            );
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
        for (device, folder, name, _) in suhina_device_registry::pack_files_flat() {
            let device_folder = dir.join(folder);
            std::fs::create_dir_all(&device_folder).unwrap();
            let target = device_folder.join(name);
            let Some(file_extension) = suhina_device_registry::file_extension(device) else { continue };
            if name.ends_with(&format!(".{file_extension}")) {
                std::fs::write(&target, b"the user's own bytes").unwrap();
            }
        }
        let written = unpack_pack(&dir);
        assert_eq!(written, 0, "the unpack wrote over a file the user carries");
        for (device, folder, name, _) in suhina_device_registry::pack_files_flat() {
            let target = dir.join(folder).join(name);
            let user = std::fs::read(&target).unwrap();
            assert_eq!(user, b"the user's own bytes", "{name} ({device:?}) was overwritten");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
