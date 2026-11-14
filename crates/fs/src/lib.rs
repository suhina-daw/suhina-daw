// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saving a file whole or not at all (#764 › 12): [`write_atomic`] writes beside the file and
//! renames over it, so a crash, a full disk or a failed write never leaves a project, a device,
//! a backup journal or the settings half written; [`write_atomic_with`] does the same for a file
//! written piece by piece. Every crate that saves a file it reads again does it through here, at
//! any layer, so this crate depends on nothing but, on Unix, `libc` for the one open flag the
//! standard library does not name. [`keep_newest`] is the one way a folder of backups, crash
//! files or caches keeps only its newest files. [`read_plain`] reads a file a project names
//! (a NAM model, an IR) only when it is a plain file, never waiting on a pipe.

use std::io::{Read, Write};
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

/// Why [`read_plain`] read nothing.
#[derive(Debug)]
pub enum ReadError {
    /// The file could not be opened or read.
    Io(std::io::Error),
    /// The path is there but is not a plain file: a folder, a named pipe, a device.
    NotAFile,
    /// The file holds more than the bytes asked for at most: its size.
    TooLarge(u64),
}

impl ReadError {
    /// The message a panel shows for `path`, read with at most `max` bytes.
    pub fn message(&self, path: &Path, max: usize) -> String {
        match self {
            ReadError::Io(e) => format!("{} ({e})", path.display()),
            ReadError::NotAFile => format!("{} is not a file", path.display()),
            ReadError::TooLarge(bytes) => {
                let megabytes = (*bytes as f64 / f64::from(1u32 << 20) * 10.0).round() / 10.0;
                format!("{} is {megabytes} MiB, larger than Suhina reads ({} MiB)", path.display(), max >> 20)
            }
        }
    }
}

/// The bytes of the plain file at `path`, at most `max` of them. The file is opened first and
/// the opened file is what is checked and read, so a file swapped for a named pipe or a folder
/// after a caller looked at the path is refused, not waited on: on Unix the open does not wait
/// for a pipe's writer (`O_NONBLOCK`, which a plain file reads the same with). A file that grows
/// past `max` while it is read is refused as well.
pub fn read_plain(path: &Path, max: usize) -> Result<Vec<u8>, ReadError> {
    let file = open_without_waiting(path).map_err(ReadError::Io)?;
    let info = file.metadata().map_err(ReadError::Io)?;
    if !info.is_file() {
        return Err(ReadError::NotAFile);
    }
    let limit = u64::try_from(max).unwrap_or(u64::MAX);
    if info.len() > limit {
        return Err(ReadError::TooLarge(info.len()));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(info.len()).unwrap_or(max).min(max));
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(ReadError::Io)?;
    if bytes.len() > max {
        return Err(ReadError::TooLarge(u64::try_from(bytes.len()).unwrap_or(u64::MAX)));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn open_without_waiting(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
}

#[cfg(not(unix))]
fn open_without_waiting(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn reads_a_plain_file_and_refuses_a_folder_and_a_large_one() {
        let dir = folder("read-plain");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("model.nam");
        std::fs::write(&file, b"twelve bytes").unwrap();
        assert_eq!(read_plain(&file, 12).unwrap(), b"twelve bytes");
        assert!(matches!(read_plain(&file, 11), Err(ReadError::TooLarge(12))));
        assert!(matches!(read_plain(&dir, 1 << 20), Err(ReadError::NotAFile)));
        assert!(matches!(read_plain(&dir.join("missing.nam"), 12), Err(ReadError::Io(_))));
        assert_eq!(
            ReadError::TooLarge(3 << 20).message(Path::new("big.nam"), 1 << 20),
            "big.nam is 3 MiB, larger than Suhina reads (1 MiB)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A named pipe with no writer is refused at once: without `O_NONBLOCK` the open would wait
    /// for a writer forever, and this test would never end.
    #[cfg(unix)]
    #[test]
    fn refuses_a_named_pipe_without_waiting_for_a_writer() {
        let dir = folder("read-pipe");
        std::fs::create_dir_all(&dir).unwrap();
        let pipe = dir.join("model.nam");
        let made = std::process::Command::new("mkfifo").arg(&pipe).status().unwrap();
        assert!(made.success());
        assert!(matches!(read_plain(&pipe, 1 << 20), Err(ReadError::NotAFile)));
        let _ = std::fs::remove_dir_all(&dir);
    }

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
