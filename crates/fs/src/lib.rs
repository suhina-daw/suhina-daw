// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saving a file whole or not at all (#764 › 12): [`write_atomic`] writes beside the file and
//! renames over it, so a crash, a full disk or a failed write never leaves a project, a device,
//! a backup journal or the settings half written; [`write_atomic_with`] does the same for a file
//! written piece by piece. Every crate that saves a file it reads again does it through here, at
//! any layer, so this crate depends on nothing. [`keep_newest`] is the one way a folder of
//! backups, crash files or caches keeps only its newest files.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Writes `bytes` to `path` whole or not at all: to `<path>.saving` beside it, flushed to the
/// disk, then renamed over `path` in one step, so a reader sees the old file or the new one.
/// Makes the folder when it is missing. On an error the file beside it is removed and `path` is
/// as it was.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_atomic_with(path, |file| file.write_all(bytes.as_ref()))
}

/// [`write_atomic`] for a file too large to build in memory first: `write` streams it into the
/// file beside `path` (buffered), and an error from `write` leaves `path` as it was, with
/// nothing beside it, as any other failed save does.
pub fn write_atomic_with(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = beside(path);
    let written = (|| {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&temporary)?);
        write(&mut out)?;
        out.into_inner().map_err(std::io::IntoInnerError::into_error)?.sync_all()
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

/// The most entries [`keep_newest`] reads from a folder, so a folder someone filled with
/// thousands of other files costs a bounded time; entries past it are left alone.
pub const MAX_LISTED: usize = 10_000;

/// Deletes all but the newest `keep` files in `folder` whose names `matches` accepts, newest by
/// the time they were modified. Files with the same modified time (a coarse clock: FAT, some
/// network shares) are ordered by name, which says nothing about their age, so which of them
/// is kept is unspecified. A missing folder, or one with `keep` or fewer such files, is left
/// alone; files `matches` refuses, folders and names that are not text are never touched. Reads
/// at most [`MAX_LISTED`] entries. Deleting is best effort: a file that cannot be deleted stays,
/// and the rest are still deleted.
pub fn keep_newest(folder: &Path, keep: usize, matches: impl Fn(&str) -> bool) {
    let Ok(listing) = std::fs::read_dir(folder) else { return };
    let mut ours: Vec<(SystemTime, PathBuf)> = listing
        .take(MAX_LISTED)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().is_some_and(&matches))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some((metadata.modified().ok()?, entry.path()))
        })
        .collect();
    if ours.len() <= keep {
        return;
    }
    ours.sort_by(|a, b| b.cmp(a));
    for (_, path) in ours.into_iter().skip(keep) {
        let _gone = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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

    #[test]
    fn a_streamed_write_saves_every_piece_and_a_failing_one_keeps_the_old_file() {
        let dir = folder("stream");
        let path = dir.join("index.tsv");
        write_atomic_with(&path, |out| (0..1000).try_for_each(|n| writeln!(out, "line {n}"))).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1000);
        assert!(text.ends_with("line 999\n"));
        let failed = write_atomic_with(&path, |out| {
            writeln!(out, "half")?;
            Err(std::io::Error::other("the disk is full"))
        });
        assert_eq!(failed.unwrap_err().to_string(), "the disk is full");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "the old file is whole");
        assert!(!beside(&path).exists(), "nothing is left beside it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writes `name` into `dir`, modified `age` seconds before a fixed time.
    fn file(dir: &Path, name: &str, age: u64) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, name).unwrap();
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000 - age);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(time).unwrap();
        path
    }

    #[test]
    fn keeps_the_newest_matching_files_and_never_touches_the_others() {
        let dir = folder("keep");
        std::fs::create_dir_all(&dir).unwrap();
        // Named against their age, so the order cannot come from the names.
        let newest = file(&dir, "log-d.txt", 1);
        let second = file(&dir, "log-c.txt", 2);
        let old = file(&dir, "log-b.txt", 3);
        let oldest = file(&dir, "log-a.txt", 4);
        let other = file(&dir, "notes.txt", 100);
        std::fs::create_dir(dir.join("log-folder.txt")).unwrap();
        keep_newest(&dir, 2, |name| name.starts_with("log-"));
        assert!(newest.exists() && second.exists());
        assert!(!old.exists() && !oldest.exists());
        assert!(other.exists(), "a file that does not match stays");
        assert!(dir.join("log-folder.txt").is_dir(), "a folder that matches stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_or_one_with_few_enough_files_is_left_alone() {
        let dir = folder("few");
        keep_newest(&dir, 2, |_| true);
        assert!(!dir.exists());
        std::fs::create_dir_all(&dir).unwrap();
        let one = file(&dir, "one", 1);
        let two = file(&dir, "two", 2);
        keep_newest(&dir, 2, |_| true);
        assert!(one.exists() && two.exists());
        keep_newest(&dir, 0, |name| name == "two");
        assert!(one.exists() && !two.exists(), "keep 0 deletes every match");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
