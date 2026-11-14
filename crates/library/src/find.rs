// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding files by name (`docs/spec/missing-files.md`): in the index of the browser's folders,
//! and by walking any folder. Names are compared without regard to case.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::Library;
use crate::index::MAX_DEPTH;

/// Entries a folder walk looks at before it stops.
pub const MAX_WALKED: usize = 1_000_000;
/// Paths kept per name.
pub const MAX_CANDIDATES: usize = 32;
/// Names asked for at once.
pub const MAX_NAMES: usize = 4096;

/// The paths found for each name asked, in the order of `names` (a name found nowhere has no
/// paths).
pub type Found = Vec<(String, Vec<PathBuf>)>;

fn key(name: &str) -> String {
    name.to_lowercase()
}

/// Gathers paths by the lower-case name they answer.
struct Gather {
    wanted: HashMap<String, Vec<PathBuf>>,
}

impl Gather {
    fn new(names: &[String]) -> Self {
        Gather { wanted: names.iter().map(|n| (key(n), Vec::new())).collect() }
    }

    fn offer(&mut self, name: &str, path: &Path) {
        if let Some(paths) = self.wanted.get_mut(&key(name))
            && paths.len() < MAX_CANDIDATES
            && !paths.iter().any(|p| p == path)
        {
            paths.push(path.to_path_buf());
        }
    }

    fn into_found(self, names: &[String]) -> Found {
        // Names are compared without case, so two names that differ only in case (two missing
        // files, "Kick.wav" in one folder and "kick.wav" in another) share their candidates.
        names.iter().map(|n| (n.clone(), self.wanted.get(&key(n)).cloned().unwrap_or_default())).collect()
    }
}

impl Library {
    /// Unique best indexed candidates for imported source paths. Equal-best names and
    /// saturated candidate lists are deliberately left for the missing-files dialog.
    pub fn resolve_import_samples(&mut self, files: &[String]) -> Vec<Option<PathBuf>> {
        if files.is_empty() {
            return Vec::new();
        }
        let names: Vec<String> = files.iter().map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_owned()).collect();
        self.find_by_names(&names)
            .into_iter()
            .zip(files)
            .map(|((_, paths), source)| {
                if paths.len() >= MAX_CANDIDATES {
                    return None;
                }
                let score = |candidate: &Path| {
                    let path = candidate.to_string_lossy().replace('\\', "/");
                    let source = source.replace('\\', "/");
                    path.rsplit('/').zip(source.rsplit('/')).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count()
                };
                let mut candidates: Vec<_> =
                    paths.into_iter().filter(|p| p.is_file()).map(|p| (score(&p), p)).collect();
                candidates.sort_by_key(|a| std::cmp::Reverse(a.0));
                let (best, path) = candidates.first()?;
                if candidates.get(1).is_some_and(|(next, _)| next == best) {
                    return None;
                }
                Some(path.clone())
            })
            .collect()
    }

    /// Files in the browser's folders with any of `names`, from the index (no disk access).
    pub fn find_by_names(&mut self, names: &[String]) -> Found {
        let mut gather = Gather::new(names);
        for file in self.files(None) {
            gather.offer(file.name(), &file.path);
        }
        gather.into_found(names)
    }
}

/// How a folder walk ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walked {
    pub found: Found,
    /// Why the walk did not cover everything (a limit, or stopped), if it did not.
    pub note: Option<String>,
}

/// Walks `root` for files with any of `names`: depth at most [`MAX_DEPTH`], at most
/// [`MAX_WALKED`] entries, symbolic links not followed. `stop` ends it early; `walked` counts
/// the entries looked at, for a progress display.
pub fn walk_for_names(
    root: &Path,
    names: &[String],
    stop: &AtomicBool,
    walked: &AtomicUsize,
) -> Result<Walked, String> {
    if names.len() > MAX_NAMES {
        return Err(format!("at most {MAX_NAMES} names at once"));
    }
    let meta = std::fs::symlink_metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let mut gather = Gather::new(names);
    let mut note = None;
    let mut pending: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    'walk: while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if stop.load(Ordering::Relaxed) {
                note = Some("stopped".to_string());
                break 'walk;
            }
            if walked.fetch_add(1, Ordering::Relaxed) >= MAX_WALKED {
                note = Some(format!("stopped after {MAX_WALKED} entries"));
                break 'walk;
            }
            // `file_type` does not follow links, so a link is neither a file nor a folder here.
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                } else if note.is_none() {
                    note = Some(format!("folders deeper than {MAX_DEPTH} levels were left out"));
                }
            } else if kind.is_file() {
                gather.offer(&entry.file_name().to_string_lossy(), &path);
            }
        }
    }
    let mut found = gather.into_found(names);
    for (_, paths) in &mut found {
        paths.sort();
    }
    Ok(Walked { found, note })
}

/// Orders `candidates` for a file that was at `last_path`: the one whose folders end like
/// `last_path`'s did (most folder names in common, from the file upwards, without regard to
/// case) first, then the shortest path, then by path.
pub fn rank_candidates(last_path: &Path, candidates: &mut [PathBuf]) {
    let folders = |p: &Path| -> Vec<String> {
        p.parent()
            .map(|d| d.components().rev().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect())
            .unwrap_or_default()
    };
    let old = folders(last_path);
    let common = |p: &Path| folders(p).iter().zip(&old).take_while(|(a, b)| a == b).count();
    candidates
        .sort_by(|a, b| common(b).cmp(&common(a)).then(a.as_os_str().len().cmp(&b.as_os_str().len())).then(a.cmp(b)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("suhina-find-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn a_walk_finds_files_by_name_in_any_case() {
        let root = temp("walk");
        touch(&root.join("Drums").join("Kicks").join("Kick 1.WAV"));
        touch(&root.join("Other").join("kick 1.wav"));
        touch(&root.join("snare.wav"));
        let names = vec!["kick 1.wav".to_string(), "hat.wav".to_string()];
        let walked = AtomicUsize::new(0);
        let result = walk_for_names(&root, &names, &AtomicBool::new(false), &walked).unwrap();
        assert_eq!(result.note, None);
        assert_eq!(result.found[0].0, "kick 1.wav");
        assert_eq!(result.found[0].1.len(), 2);
        assert!(result.found[1].1.is_empty());
        assert!(walked.load(Ordering::Relaxed) >= 5);
        // Stopping ends the walk at once.
        let stopped = walk_for_names(&root, &names, &AtomicBool::new(true), &AtomicUsize::new(0)).unwrap();
        assert_eq!(stopped.note.as_deref(), Some("stopped"));
        assert!(walk_for_names(&root.join("nothing"), &names, &AtomicBool::new(false), &AtomicUsize::new(0)).is_err());
        assert!(
            walk_for_names(&root.join("snare.wav"), &names, &AtomicBool::new(false), &AtomicUsize::new(0)).is_err()
        );
        let too_many = vec![String::new(); MAX_NAMES + 1];
        assert!(walk_for_names(&root, &too_many, &AtomicBool::new(false), &AtomicUsize::new(0)).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn candidates_with_the_same_folders_come_first() {
        let mut candidates = vec![
            PathBuf::from("/new/Other/kick.wav"),
            PathBuf::from("/a/very/long/new/place/Drums/Kicks/kick.wav"),
            PathBuf::from("/new/Kicks/kick.wav"),
            PathBuf::from("/x/kick.wav"),
        ];
        rank_candidates(Path::new("/old/Drums/Kicks/kick.wav"), &mut candidates);
        assert_eq!(candidates[0], PathBuf::from("/a/very/long/new/place/Drums/Kicks/kick.wav"));
        assert_eq!(candidates[1], PathBuf::from("/new/Kicks/kick.wav"));
        assert_eq!(candidates[2], PathBuf::from("/x/kick.wav"));
    }

    #[test]
    fn imported_samples_use_unique_trailing_paths_and_leave_ties_unresolved() {
        let settings = temp("import-settings");
        let samples = temp("import-samples");
        touch(&samples.join("Pack/AMB/Voice.WAV"));
        touch(&samples.join("Other/Voice.WAV"));
        touch(&samples.join("Only/Unique.wav"));
        let mut library = Library::new(Some(settings));
        library.add_folder(&samples).unwrap();
        let found = library.resolve_import_samples(&[
            "D:\\Old\\Pack\\AMB\\voice.wav".into(),
            "Voice.WAV".into(),
            "old/Unique.wav".into(),
            "absent.wav".into(),
        ]);
        assert_eq!(found[0], Some(samples.join("Pack/AMB/Voice.WAV")));
        assert_eq!(found[1], None, "equal filename matches must not be guessed");
        assert_eq!(found[2], Some(samples.join("Only/Unique.wav")));
        assert_eq!(found[3], None);
        for i in 0..MAX_CANDIDATES {
            touch(&samples.join(format!("{i}/Many.wav")));
        }
        let mut library = Library::new(None);
        library.add_folder(&samples).unwrap();
        assert_eq!(
            library.resolve_import_samples(&["0/Many.wav".into()]),
            vec![None],
            "a truncated candidate list cannot establish a unique best match"
        );
    }

    #[test]
    fn the_index_answers_names() {
        let settings = temp("index-settings");
        let samples = temp("index-samples");
        touch(&samples.join("Loops").join("Groove.wav"));
        touch(&samples.join("snare.wav"));
        let mut library = Library::new(Some(settings.clone()));
        library.add_folder(&samples).unwrap();
        let found =
            library.find_by_names(&["groove.WAV".to_string(), "none.wav".to_string(), "Groove.wav".to_string()]);
        assert_eq!(found[0].1, vec![samples.join("Loops").join("Groove.wav")]);
        assert!(found[1].1.is_empty());
        // A second name that differs only in case finds the same file; it was left with none.
        assert_eq!(found[2].1, found[0].1);
        let _ = std::fs::remove_dir_all(&settings);
        let _ = std::fs::remove_dir_all(&samples);
    }
}
