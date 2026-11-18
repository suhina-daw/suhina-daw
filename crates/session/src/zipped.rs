// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A project packed in a zip with its samples, as another DAW writes one to send a song
//! (`docs/spec/missing-files.md` › Zipped projects): [`zipped_project`] looks inside for the
//! project, [`extract_zipped_project`] unpacks it into a folder the person chose, and the project
//! then opens from there like any other, its samples beside it.
//!
//! The zip is someone else's file: every entry's name is checked to stay inside the folder,
//! links and devices are not unpacked, nothing already there is written over, and the number of
//! files and the bytes they unpack to are capped before and while they are written.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// The project files a zip may carry, by extension.
const PROJECTS: [&str; 2] = ["als", "suhina"];
/// At most this many entries in the zip.
const MAX_ENTRIES: usize = 10_000;
/// At most this many bytes unpacked, all entries together.
const MAX_UNPACKED: u64 = 8 * 1024 * 1024 * 1024;
/// A zip file larger than this is not looked into.
const MAX_ZIP: u64 = 8 * 1024 * 1024 * 1024;

/// What a zip holds when it holds a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZippedProject {
    /// The project's name inside the zip, `/`-separated.
    pub project: String,
    /// Files in the zip, the project included.
    pub files: usize,
    /// The bytes they unpack to, as the zip states them.
    pub bytes: u64,
}

fn is_project(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PROJECTS.iter().any(|p| e.eq_ignore_ascii_case(p)))
}

fn open(zip: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let size = file.metadata().map_err(|e| format!("{}: {e}", zip.display()))?.len();
    if size > MAX_ZIP {
        return Err(format!("{}: the zip is larger than {} GiB", zip.display(), MAX_ZIP >> 30));
    }
    let archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: not a readable zip ({e})", zip.display()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!("{}: the zip holds more than {MAX_ENTRIES} entries", zip.display()));
    }
    Ok(archive)
}

/// The project a zip holds, from its entries' names alone; `None` when it holds none (a zip of
/// themes, for example). Two or more projects are an error: which one to open is not ours to guess.
pub fn zipped_project(zip: &Path) -> Result<Option<ZippedProject>, String> {
    let mut archive = open(zip)?;
    let mut projects = Vec::new();
    let mut files = 0;
    let mut bytes: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|e| format!("{}: {e}", zip.display()))?;
        if entry.is_dir() {
            continue;
        }
        files += 1;
        bytes = bytes.saturating_add(entry.size());
        let name = entry.name().map_err(|e| format!("{}: {e}", zip.display()))?.replace('\\', "/");
        if is_project(&name) {
            projects.push(name);
        }
    }
    match projects.as_slice() {
        [] => Ok(None),
        [project] => Ok(Some(ZippedProject { project: project.clone(), files, bytes })),
        _ => Err(format!(
            "{}: the zip holds {} projects; unpack it and open the one you want",
            zip.display(),
            projects.len()
        )),
    }
}

/// Where a zip is unpacked unless the person says otherwise: a folder named after it, beside it,
/// with a number added when one of that name is already there and not empty.
pub fn default_extract_folder(zip: &Path) -> PathBuf {
    let parent = zip.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = zip.file_stem().map_or_else(|| "Project".into(), |s| s.to_string_lossy().into_owned());
    let free = |path: &Path| std::fs::read_dir(path).map_or(true, |mut entries| entries.next().is_none());
    let first = parent.join(&stem);
    if free(&first) {
        return first;
    }
    (2..1000).map(|n| parent.join(format!("{stem} {n}"))).find(|p| free(p)).unwrap_or(first)
}

/// `name` from the zip as a path inside `into`, or `None` when it would step out of it, name a
/// root, a drive or a device, or hold no file name.
fn inside(into: &Path, name: &Path) -> Option<PathBuf> {
    let mut path = into.to_path_buf();
    let mut parts = 0;
    for part in name.components() {
        match part {
            Component::Normal(part) => {
                path.push(part);
                parts += 1;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (parts > 0).then_some(path)
}

/// Unpacks the project zip `zip` into the folder `into` (made when it is not there) and returns
/// the path of the project in it. A file already in `into` is never written over: the unpack
/// stops with an error before it, leaving what was written. Links, devices and names that would
/// leave `into` are refused, and the files written are capped as a whole.
pub fn extract_zipped_project(zip: &Path, into: &Path) -> Result<PathBuf, String> {
    let Some(found) = zipped_project(zip)? else {
        return Err(format!("{}: the zip holds no project (.als or .suhina)", zip.display()));
    };
    if std::fs::symlink_metadata(into).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{}: the folder to unpack into is a link", into.display()));
    }
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let mut archive = open(zip)?;
    let mut written: u64 = 0;
    let mut project = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("{}: {e}", zip.display()))?;
        let name = entry.name().map_err(|e| format!("{}: {e}", zip.display()))?.replace('\\', "/");
        if entry.is_symlink() {
            return Err(format!("{name}: the zip holds a link, which is not unpacked"));
        }
        let Some(target) = entry.enclosed_name().and_then(|n| inside(into, &n)) else {
            return Err(format!("{name}: a name that leaves the folder, which is not unpacked"));
        };
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!("{}: already there, and not written over", target.display())
            } else {
                format!("{}: {e}", target.display())
            }
        })?;
        // The stated size is not trusted: what is read is capped by what is left of the budget.
        let left = MAX_UNPACKED.saturating_sub(written);
        let copied = std::io::copy(&mut (&mut entry).take(left.saturating_add(1)), &mut file)
            .map_err(|e| format!("{name}: {e}"))?;
        if copied > left {
            drop(file);
            let _ = std::fs::remove_file(&target);
            return Err(format!("the zip unpacks to more than {} GiB, which is not unpacked", MAX_UNPACKED >> 30));
        }
        file.flush().map_err(|e| format!("{}: {e}", target.display()))?;
        written += copied;
        if name == found.project {
            project = Some(target);
        }
    }
    project.ok_or_else(|| format!("{}: the project was not unpacked", found.project))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-zipped-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn a_zip_with_one_project_is_found_and_unpacked_beside_its_samples() {
        let dir = folder("one");
        let zip = dir.join("Song.zip");
        zip_of(&zip, &[("Song.als", b"FLhd"), ("Kick.wav", b"RIFF"), ("Samples/Snare.wav", b"RIFF")]);
        let found = zipped_project(&zip).unwrap().unwrap();
        assert_eq!((found.project.as_str(), found.files, found.bytes), ("Song.als", 3, 12));
        assert_eq!(default_extract_folder(&zip), dir.join("Song"));
        let project = extract_zipped_project(&zip, &dir.join("Song")).unwrap();
        assert_eq!(project, dir.join("Song").join("Song.als"));
        assert_eq!(std::fs::read(dir.join("Song/Samples/Snare.wav")).unwrap(), b"RIFF");
        // Unpacked once, the folder is taken: the default moves on, and a second unpack there
        // writes nothing over.
        assert_eq!(default_extract_folder(&zip), dir.join("Song 2"));
        let again = extract_zipped_project(&zip, &dir.join("Song")).unwrap_err();
        assert!(again.contains("not written over"), "{again}");
        assert_eq!(std::fs::read(dir.join("Song/Song.als")).unwrap(), b"FLhd");
    }

    #[test]
    fn a_zip_without_a_project_or_with_two_is_not_one_to_open() {
        let dir = folder("none");
        let themes = dir.join("themes.zip");
        zip_of(&themes, &[("Night/theme.css", b"a"), ("Night/theme.json", b"{}")]);
        assert_eq!(zipped_project(&themes).unwrap(), None);
        assert!(extract_zipped_project(&themes, &dir.join("out")).is_err());
        let two = dir.join("two.zip");
        zip_of(&two, &[("A.suhina", b"x"), ("B.als", b"y")]);
        assert!(zipped_project(&two).unwrap_err().contains("2 projects"));
        std::fs::write(dir.join("not.zip"), b"not a zip").unwrap();
        assert!(zipped_project(&dir.join("not.zip")).is_err());
    }

    #[test]
    fn names_that_leave_the_folder_are_refused_and_nothing_lands_outside() {
        let dir = folder("slip");
        let zip = dir.join("slip.zip");
        zip_of(&zip, &[("Song.als", b"FLhd"), ("../escaped.wav", b"RIFF")]);
        let error = extract_zipped_project(&zip, &dir.join("out")).unwrap_err();
        assert!(error.contains("leaves the folder"), "{error}");
        assert!(!dir.join("escaped.wav").exists());
        assert!(inside(&dir, Path::new("/etc/passwd")).is_none());
        assert!(inside(&dir, Path::new("a/../../b")).is_none());
        assert!(inside(&dir, Path::new(".")).is_none());
        assert_eq!(inside(&dir, Path::new("./a/b.wav")), Some(dir.join("a").join("b.wav")));
    }
}
