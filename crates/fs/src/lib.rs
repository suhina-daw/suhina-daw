// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saving a file whole or not at all (#764 › 12): [`write_atomic`] writes beside the file and
//! renames over it, so a crash, a full disk or a failed write never leaves a project, a device,
//! a backup journal or the settings half written. Every crate that saves a file it reads again
//! does it through here, at any layer, so this crate depends on nothing.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` whole or not at all: to `<path>.saving` beside it, flushed to the
/// disk, then renamed over `path` in one step, so a reader sees the old file or the new one.
/// Makes the folder when it is missing. On an error the file beside it is removed and `path` is
/// as it was.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = beside(path);
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes.as_ref())?;
        file.sync_all()
    })()
    .and_then(|()| std::fs::rename(&temporary, path));
    written.inspect_err(|_| {
        let _gone = std::fs::remove_file(&temporary);
    })
}

/// The file [`write_atomic`] writes before it renames it: `path` with `.saving` added.
pub fn beside(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".saving");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_the_file_and_its_missing_folder_and_leaves_nothing_beside_it() {
        let dir = folder("write");
        let path = dir.join("deep").join("project.suhina");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!beside(&path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_rename_keeps_the_old_file_and_removes_the_one_beside_it() {
        let dir = folder("fail");
        // A folder where the file should go: the rename over it fails.
        let path = dir.join("taken");
        std::fs::create_dir_all(path.join("inside")).unwrap();
        assert!(write_atomic(&path, b"new").is_err());
        assert!(path.join("inside").is_dir());
        assert!(!beside(&path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
