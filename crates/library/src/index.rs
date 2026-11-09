// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One folder's index (ADR 0012): the files of the kinds the browser lists and every
//! sub-folder, with their sizes and modification times. Built by walking the folder within the
//! limits, brought up to date by reading only the sub-folders whose time changed, and kept on
//! disk as tab-separated lines.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use suhina_media::info::{FileKind, kind_of};

/// Files indexed per folder.
pub const MAX_FILES: usize = 200_000;
/// Sub-folders deeper than this are left out.
pub const MAX_DEPTH: usize = 32;
/// Longest name (of a file or folder) indexed, in bytes.
pub const MAX_NAME: usize = 1024;
/// Largest index file read back.
const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
const HEADER: &str = "suhina-library-index 2";
/// The same format, written before the program was renamed.
const OLD_HEADER: &str = "loopwerk-library-index 2"; // rename_suhina: keep

/// A file in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: FileKind,
    pub size: u64,
    /// Nanoseconds since 1970.
    pub modified: u64,
}

/// One folder's index. Paths inside are relative to the folder, with `/` between parts; the
/// folder itself is `""`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderIndex {
    pub files: BTreeMap<String, Entry>,
    /// Every sub-folder (and the folder itself) with its modification time.
    pub dirs: BTreeMap<String, u64>,
    /// Why the index is not complete, when a limit was reached.
    pub note: Option<String>,
}

/// Nanoseconds since 1970: folder times must tell apart changes within one second.
fn nanos(time: std::io::Result<std::time::SystemTime>) -> u64 {
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// The modification time of the folder at `path`, or `None` when it is not a folder (links are
/// not followed).
pub fn dir_time(path: &Path) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    meta.is_dir().then(|| nanos(meta.modified()))
}

fn join(rel: &str, name: &str) -> String {
    if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") }
}

fn depth(rel: &str) -> usize {
    if rel.is_empty() { 0 } else { rel.matches('/').count() + 1 }
}

/// Whether a name can be kept in the index (not too long, no tab or line break).
fn usable(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && !name.contains(['\t', '\n', '\r'])
}

/// The absolute path of `rel` inside `root`.
pub fn path_of(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() { root.to_path_buf() } else { rel.split('/').fold(root.to_path_buf(), |p, part| p.join(part)) }
}

impl FolderIndex {
    /// Walks `root` within the limits.
    pub fn scan(root: &Path) -> Self {
        let mut index = Self::default();
        index.read_tree(root, "");
        index
    }

    /// Whether the index is full.
    fn full(&self) -> bool {
        self.files.len() >= MAX_FILES
    }

    /// Reads the folder `rel` and everything below it.
    fn read_tree(&mut self, root: &Path, rel: &str) {
        let mut pending = vec![rel.to_string()];
        while let Some(dir) = pending.pop() {
            let found = self.read_dir(root, &dir);
            pending.extend(found);
        }
    }

    /// Reads what is directly in the folder `rel`: its files replace the ones indexed there, and
    /// the sub-folders it holds are returned (new and old). Sub-folders that are gone are
    /// removed with everything below them.
    fn read_dir(&mut self, root: &Path, rel: &str) -> Vec<String> {
        let path = path_of(root, rel);
        let Some(time) = dir_time(&path) else {
            self.remove_tree(rel);
            return Vec::new();
        };
        self.dirs.insert(rel.to_string(), time);
        let Ok(entries) = std::fs::read_dir(&path) else { return Vec::new() };
        // The files indexed directly in this folder, to drop the ones that are gone.
        let prefix = if rel.is_empty() { String::new() } else { format!("{rel}/") };
        let mut old_files: Vec<String> = self
            .files
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .filter(|(k, _)| !k[prefix.len()..].contains('/'))
            .map(|(k, _)| k.clone())
            .collect();
        let mut old_dirs: Vec<String> = self
            .dirs
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .filter(|(k, _)| !k.is_empty() && *k != rel && !k[prefix.len()..].contains('/'))
            .map(|(k, _)| k.clone())
            .collect();
        let mut sub = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if !usable(&name) {
                continue;
            }
            // `file_type` does not follow links, so a link is neither a file nor a folder here.
            let Ok(kind) = entry.file_type() else { continue };
            let child = join(rel, &name);
            if kind.is_dir() {
                if depth(&child) <= MAX_DEPTH {
                    old_dirs.retain(|d| *d != child);
                    sub.push(child);
                } else if self.note.is_none() {
                    self.note = Some(format!("folders deeper than {MAX_DEPTH} levels are left out"));
                }
            } else if kind.is_file()
                && let Some(file_kind) = kind_of(Path::new(&name))
            {
                old_files.retain(|f| *f != child);
                if !self.files.contains_key(&child) && self.full() {
                    self.note = Some(format!("only the first {MAX_FILES} files are indexed"));
                    continue;
                }
                let meta = entry.metadata().ok();
                let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                let modified = meta.map_or(0, |m| nanos(m.modified()));
                self.files.insert(child, Entry { kind: file_kind, size, modified });
            }
        }
        for gone in old_files {
            self.files.remove(&gone);
        }
        for gone in old_dirs {
            self.remove_tree(&gone);
        }
        sub
    }

    /// Removes the folder `rel` and everything below it.
    fn remove_tree(&mut self, rel: &str) {
        if rel.is_empty() {
            self.files.clear();
            self.dirs.clear();
            return;
        }
        let prefix = format!("{rel}/");
        self.files.retain(|k, _| !k.starts_with(&prefix));
        self.dirs.retain(|k, _| k != rel && !k.starts_with(&prefix));
    }

    /// Brings the index up to date with the disk: every known folder whose time changed is read
    /// again (new sub-folders in it are walked), every one that is gone is removed. Returns
    /// whether anything changed.
    pub fn refresh(&mut self, root: &Path) -> bool {
        let before = (self.files.len(), self.dirs.len());
        let snapshot = self.clone();
        let known: Vec<(String, u64)> = self.dirs.iter().map(|(k, v)| (k.clone(), *v)).collect();
        if known.is_empty() {
            self.read_tree(root, "");
        }
        for (rel, time) in known {
            if !self.dirs.contains_key(&rel) {
                continue; // removed with a parent
            }
            match dir_time(&path_of(root, &rel)) {
                None => self.remove_tree(&rel),
                Some(now) if now != time => {
                    let sub = self.read_dir(root, &rel);
                    for child in sub {
                        if !self.dirs.contains_key(&child) {
                            self.read_tree(root, &child);
                        }
                    }
                }
                Some(_) => {}
            }
        }
        before != (self.files.len(), self.dirs.len()) || *self != snapshot
    }

    /// Reads again the folders the paths named in a change event are in (or are), and walks
    /// new sub-folders. Paths outside `root` are ignored.
    pub fn update_paths(&mut self, root: &Path, paths: &[PathBuf]) -> bool {
        let snapshot = self.clone();
        let mut dirs: Vec<String> = Vec::new();
        for path in paths {
            let Ok(rest) = path.strip_prefix(root) else { continue };
            let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            let rel = parts.join("/");
            let target =
                if dir_time(path).is_some() { rel.clone() } else { parts[..parts.len().saturating_sub(1)].join("/") };
            if !dirs.contains(&target) {
                dirs.push(target);
            }
            // A file that changed in place keeps its folder's time; its own entry is updated.
            if let Some(entry) = self.files.get_mut(&rel)
                && let Ok(meta) = std::fs::symlink_metadata(path)
            {
                entry.size = meta.len();
                entry.modified = nanos(meta.modified());
            }
        }
        for dir in dirs {
            if depth(&dir) > MAX_DEPTH {
                continue;
            }
            // A folder not known yet whose parent is known is walked whole.
            let sub = self.read_dir(root, &dir);
            for child in sub {
                if !self.dirs.contains_key(&child) {
                    self.read_tree(root, &child);
                }
            }
        }
        *self != snapshot
    }

    /// Writes the index for `root` to `file` whole or not at all, through `suhina-fs`: a save
    /// that fails leaves the old index as it was, with nothing beside it.
    pub fn save(&self, root: &Path, file: &Path) -> std::io::Result<()> {
        suhina_fs::write_atomic_with(file, |out| {
            writeln!(out, "{HEADER}")?;
            writeln!(out, "root\t{}", root.display())?;
            if let Some(note) = &self.note {
                writeln!(out, "note\t{}", note.replace(['\t', '\n', '\r'], " "))?;
            }
            for (rel, time) in &self.dirs {
                writeln!(out, "d\t{rel}\t{time}")?;
            }
            for (rel, e) in &self.files {
                writeln!(out, "f\t{rel}\t{}\t{}\t{}", e.kind.name(), e.size, e.modified)?;
            }
            Ok(())
        })
    }

    /// Reads an index saved for `root`; `None` when the file is missing, too large, of another
    /// version or for another folder. Lines that do not parse are skipped.
    pub fn load(root: &Path, file: &Path) -> Option<Self> {
        let size = std::fs::metadata(file).ok()?.len();
        if size > MAX_INDEX_BYTES {
            return None;
        }
        let text = std::fs::read_to_string(file).ok()?;
        let mut lines = text.lines();
        if !matches!(lines.next()?, HEADER | OLD_HEADER) {
            return None;
        }
        let saved_root = lines.next()?.strip_prefix("root\t")?;
        if Path::new(saved_root) != root {
            return None;
        }
        let mut index = Self::default();
        for line in lines {
            let mut parts = line.split('\t');
            match parts.next() {
                Some("note") => index.note = parts.next().map(str::to_string),
                Some("d") => {
                    let (Some(rel), Some(time)) = (parts.next(), parts.next().and_then(|t| t.parse().ok())) else {
                        continue;
                    };
                    if depth(rel) <= MAX_DEPTH && rel.len() <= MAX_NAME * 4 {
                        index.dirs.insert(rel.to_string(), time);
                    }
                }
                Some("f") => {
                    let rel = parts.next();
                    let kind = parts.next().and_then(FileKind::parse);
                    let size = parts.next().and_then(|s| s.parse().ok());
                    let modified = parts.next().and_then(|s| s.parse().ok());
                    if let (Some(rel), Some(kind), Some(size), Some(modified)) = (rel, kind, size, modified)
                        && !rel.is_empty()
                        && index.files.len() < MAX_FILES
                    {
                        index.files.insert(rel.to_string(), Entry { kind, size, modified });
                    }
                }
                _ => {}
            }
        }
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn indexes_files_of_known_kinds_and_every_folder() {
        let root = temp("scan");
        touch(&root.join("Kick.wav"));
        touch(&root.join("Loops/Drums/break 120bpm.flac"));
        touch(&root.join("Loops/readme.txt"));
        touch(&root.join("MIDI/groove.MID"));
        touch(&root.join("bank.syx"));
        std::fs::create_dir_all(root.join("Empty")).unwrap();
        let index = FolderIndex::scan(&root);
        let files: Vec<(&str, FileKind)> = index.files.iter().map(|(k, e)| (k.as_str(), e.kind)).collect();
        assert_eq!(
            files,
            [
                ("Kick.wav", FileKind::Sample),
                ("Loops/Drums/break 120bpm.flac", FileKind::Sample),
                ("MIDI/groove.MID", FileKind::Midi),
                ("bank.syx", FileKind::Preset),
            ]
        );
        let dirs: Vec<&str> = index.dirs.keys().map(String::as_str).collect();
        assert_eq!(dirs, ["", "Empty", "Loops", "Loops/Drums", "MIDI"]);
        assert_eq!(index.note, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn saves_and_loads_and_refuses_another_folders_or_a_damaged_index() {
        let root = temp("save");
        touch(&root.join("a/b.wav"));
        let index = FolderIndex::scan(&root);
        let file = root.join("index.tsv");
        index.save(&root, &file).unwrap();
        assert_eq!(FolderIndex::load(&root, &file), Some(index.clone()));
        assert_eq!(FolderIndex::load(&root.join("a"), &file), None, "another folder's");
        std::fs::write(&file, "something else\n").unwrap();
        assert_eq!(FolderIndex::load(&root, &file), None);
        std::fs::write(
            &file,
            format!(
                "{HEADER}\nroot\t{}\nf\tx.wav\tsample\tnot-a-size\t0\nf\ty.wav\tsample\t3\t4\nzzz\n",
                root.display()
            ),
        )
        .unwrap();
        let loaded = FolderIndex::load(&root, &file).unwrap();
        assert_eq!(loaded.files.keys().collect::<Vec<_>>(), ["y.wav"], "bad lines are skipped");
        let saved = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, saved.replacen(HEADER, OLD_HEADER, 1)).unwrap();
        assert_eq!(FolderIndex::load(&root, &file), Some(loaded), "an index from before the rename");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A save that cannot write (its folder made read-only) leaves the old index whole and no
    /// partial file beside it.
    #[cfg(unix)]
    #[test]
    fn a_failed_save_keeps_the_old_index_and_leaves_nothing_beside_it() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("failed-save");
        touch(&root.join("a.wav"));
        let index = FolderIndex::scan(&root);
        let folder = root.join("indexes");
        let file = folder.join("index.tsv");
        index.save(&root, &file).unwrap();
        let old = std::fs::read(&file).unwrap();
        let mut grown = index.clone();
        grown.note = Some("a new note".into());
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let failed = grown.save(&root, &file);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), old, "the old index is whole");
        let names: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["index.tsv"], "nothing is left beside it");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refresh_reads_changed_folders_and_drops_removed_ones() {
        let root = temp("refresh");
        touch(&root.join("keep/one.wav"));
        touch(&root.join("gone/two.wav"));
        let mut index = FolderIndex::scan(&root);
        assert!(!index.refresh(&root), "nothing changed");
        std::fs::remove_dir_all(root.join("gone")).unwrap();
        touch(&root.join("new/deep/three.mid"));
        // Folder times have a resolution of a second on some systems; the root's entry list
        // changed, so make sure its time moves.
        index.dirs.insert(String::new(), 0);
        assert!(index.refresh(&root));
        let files: Vec<&str> = index.files.keys().map(String::as_str).collect();
        assert_eq!(files, ["keep/one.wav", "new/deep/three.mid"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_event_updates_the_folder_it_names() {
        let root = temp("event");
        touch(&root.join("pack/a.wav"));
        let mut index = FolderIndex::scan(&root);
        touch(&root.join("pack/b.wav"));
        touch(&root.join("pack/sub/c.wav"));
        assert!(index.update_paths(&root, &[root.join("pack/b.wav"), root.join("pack/sub")]));
        assert_eq!(index.files.keys().collect::<Vec<_>>(), ["pack/a.wav", "pack/b.wav", "pack/sub/c.wav"]);
        std::fs::remove_file(root.join("pack/a.wav")).unwrap();
        assert!(index.update_paths(&root, &[root.join("pack/a.wav"), std::env::temp_dir().join("elsewhere")]));
        assert!(!index.files.contains_key("pack/a.wav"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deep_and_huge_folders_stop_at_the_limits() {
        let root = temp("deep");
        let mut deep = root.clone();
        for i in 0..MAX_DEPTH + 3 {
            deep = deep.join(format!("d{i}"));
        }
        touch(&deep.join("lost.wav"));
        touch(&root.join("d0/found.wav"));
        let index = FolderIndex::scan(&root);
        assert!(index.files.contains_key("d0/found.wav"));
        assert!(!index.files.keys().any(|k| k.ends_with("lost.wav")));
        assert!(index.note.is_some());
        assert!(index.dirs.keys().all(|k| depth(k) <= MAX_DEPTH));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_folder_has_an_empty_index() {
        let root = std::env::temp_dir().join("suhina-index-does-not-exist");
        let mut index = FolderIndex::scan(&root);
        assert!(index.files.is_empty() && index.dirs.is_empty());
        assert!(!index.refresh(&root));
    }
}
