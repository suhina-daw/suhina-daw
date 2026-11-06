// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The projects opened and saved lately, newest first, kept in `recent.txt` (one path a line)
//! beside the settings file, for the project dialog.

use std::path::{Path, PathBuf};

/// How many are kept.
const KEPT: usize = 12;
/// Larger files are not a list of recent projects.
const MAX_FILE: u64 = 64 * 1024;

pub struct Recent {
    file: Option<PathBuf>,
    paths: Vec<PathBuf>,
}

impl Recent {
    /// The list kept in `file`, empty when there is none yet or it cannot be read.
    pub fn load(file: Option<PathBuf>) -> Self {
        let text = file
            .as_ref()
            .filter(|f| std::fs::metadata(f).is_ok_and(|m| m.len() <= MAX_FILE))
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default();
        let paths = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).take(KEPT).collect();
        Self { file, paths }
    }

    /// Writes the list to `file` from now on (the data folder moved).
    pub fn set_file(&mut self, file: Option<PathBuf>) {
        self.file = file;
    }

    /// Puts `path` first and writes the list.
    pub fn add(&mut self, path: &Path) -> Result<(), String> {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let same = |p: &PathBuf| p.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy());
        self.paths.retain(|p| !same(p));
        self.paths.insert(0, path);
        self.paths.truncate(KEPT);
        self.write()
    }

    /// Takes `path` off the list and writes it.
    pub fn remove(&mut self, path: &Path) -> Result<(), String> {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.paths.retain(|p| !p.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy()));
        self.write()
    }

    fn write(&self) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        let text: String = self.paths.iter().map(|p| format!("{}\n", p.display())).collect();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::save_error::message("the recent projects", &e))?;
        }
        std::fs::write(file, text).map_err(|e| crate::save_error::message("the recent projects", &e))
    }

    /// The ones that are still there, newest first.
    pub fn existing(&self) -> Vec<PathBuf> {
        self.paths.iter().filter(|p| p.is_file()).cloned().collect()
    }
}

/// The projects in the templates folder, Suhina projects and Live Sets, by name.
pub fn templates(folder: &Path) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<PathBuf> = dir
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && (suhina_session::is_project_file(p) || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("als")))
        })
        .take(1000)
        .collect();
    found.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_comes_first_and_doubles_go() {
        let dir = std::env::temp_dir().join(format!("suhina-recent-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let file = dir.join("recent.txt");
        let songs: Vec<PathBuf> = (0..14).map(|i| dir.join(format!("song {i}.suhina"))).collect();
        for song in &songs {
            std::fs::write(song, "x").unwrap_or_default();
        }
        let mut recent = Recent::load(Some(file.clone()));
        for song in &songs {
            assert_eq!(recent.add(song), Ok(()));
        }
        assert_eq!(recent.add(&songs[13]), Ok(()));
        let again = Recent::load(Some(file));
        let listed = again.existing();
        assert_eq!(listed.len(), KEPT);
        assert_eq!(listed.first(), Some(&songs[13]));
        assert_eq!(listed.get(1), Some(&songs[12]));
        // A file that is gone is not listed.
        let _gone = std::fs::remove_file(&songs[12]);
        assert_eq!(again.existing().get(1), Some(&songs[11]));
        // One taken off the list stays off when it is read again.
        let mut again = again;
        assert_eq!(again.remove(&songs[11]), Ok(()));
        let read = Recent::load(Some(dir.join("recent.txt")));
        assert_eq!(read.existing().get(1), Some(&songs[10]));
        let _clean = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn templates_are_projects_by_name() {
        let dir = std::env::temp_dir().join(format!("suhina-templates-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_default();
        for name in ["b.suhina", "A.als", "notes.txt", "c.SUHINA"] {
            std::fs::write(dir.join(name), "x").unwrap_or_default();
        }
        let names: Vec<String> =
            templates(&dir).iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["A.als", "b.suhina", "c.SUHINA"]);
        assert!(templates(&dir.join("missing")).is_empty());
        let _clean = std::fs::remove_dir_all(&dir);
    }
}
