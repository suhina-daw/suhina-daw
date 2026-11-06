// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where Suhina keeps its data when the person has chosen a folder of their own (Settings ›
//! General › Settings folder). The settings live in that folder, so the choice cannot be one of
//! them: a small file in the default folder, `data-location.txt`, names it.

use std::path::{Path, PathBuf};

/// The file in the default data folder that names the folder in use.
const POINTER: &str = "data-location.txt";
/// What is copied into a new folder: the settings, keys, recent projects and themes.
const FILES: &[&str] = &["settings.txt", "keymap.json", "recent.txt"];
const THEMES: &str = "themes";
/// The most files and folders a copy walks, so a wrong folder cannot keep it busy.
const MAX_ENTRIES: usize = 20_000;

/// The folder `default`'s pointer names, when it names one that is there.
pub fn pointed(default: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(default.join(POINTER)).ok()?;
    let folder = PathBuf::from(text.trim());
    (!text.trim().is_empty() && folder.is_absolute() && folder.is_dir()).then_some(folder)
}

/// Points `default` at `folder` from the next start, or back at itself with `None`.
pub fn point(default: &Path, folder: Option<&Path>) -> Result<(), String> {
    let pointer = default.join(POINTER);
    match folder {
        Some(folder) => {
            std::fs::create_dir_all(default).map_err(|e| format!("could not make {}: {e}", default.display()))?;
            std::fs::write(&pointer, folder.display().to_string())
                .map_err(|e| format!("could not write {}: {e}", pointer.display()))
        }
        None => match std::fs::remove_file(&pointer) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("could not remove {}: {e}", pointer.display()))
            }
            _ => Ok(()),
        },
    }
}

/// Copies the settings, keys, recent projects and themes from `from` into `to`, made first,
/// leaving every file `to` has already as it is. Links are not followed. Returns how many files
/// were copied.
#[cfg(test)]
pub fn copy_over(from: &Path, to: &Path) -> Result<usize, String> {
    copy_over_keeping(from, to, &mut Vec::new())
}

/// Copies the settings, keys, recent projects and themes from `from` into `to`, as `copy_over`
/// does, adding to `links` each link it leaves where it is.
fn copy_over_keeping(from: &Path, to: &Path, links: &mut Vec<PathBuf>) -> Result<usize, String> {
    if from == to {
        return Ok(0);
    }
    std::fs::create_dir_all(to).map_err(|e| format!("could not make {}: {e}", to.display()))?;
    let mut copied = 0;
    let mut seen = 0;
    for name in FILES {
        copied += copy_file(&from.join(name), &to.join(name))?;
    }
    copy_tree(&from.join(THEMES), &to.join(THEMES), &mut copied, &mut seen, links)?;
    Ok(copied)
}

/// Moves Suhina's data (the settings, keys, recent projects and themes; `only_themes` for the
/// themes folder alone) from `from` into `to`: a backup of it is made first, beside `from`
/// (`<name>-backup-<date and time>`), then it is copied into `to`, keeping what `to` has
/// already, and only then taken out of `from`. Returns the backup's folder and how many files
/// moved.
pub fn move_data(from: &Path, to: &Path, only_themes: bool) -> Result<(PathBuf, usize), String> {
    if from == to || to.starts_with(from) {
        return Err(format!("{} is inside the folder it would move from", to.display()));
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let name = from.file_name().map_or_else(|| "suhina".into(), |n| n.to_string_lossy().into_owned());
    let backup = from.with_file_name(format!("{name}-backup-{stamp}"));
    let mut copied = 0;
    let mut seen = 0;
    // Links (a theme folder that is a junction or symlink elsewhere) are not copied, so they stay
    // where they are: nothing is taken out of `from` that `to` and the backup do not hold.
    let mut links = Vec::new();
    if only_themes {
        copy_tree(from, &backup, &mut copied, &mut seen, &mut Vec::new())?;
        copied = 0;
        seen = 0;
        copy_tree(from, to, &mut copied, &mut seen, &mut links)?;
        remove_except(from, &links)?;
    } else {
        copy_over_keeping(from, &backup, &mut Vec::new())?;
        copied = copy_over_keeping(from, to, &mut links)?;
        for name in FILES {
            remove_file(&from.join(name))?;
        }
        let themes = from.join(THEMES);
        if std::fs::symlink_metadata(&themes).is_ok_and(|m| m.is_dir()) {
            remove_except(&themes, &links)?;
            if !links.iter().any(|link| link.starts_with(&themes)) {
                std::fs::remove_dir(&themes).map_err(|e| format!("could not remove {}: {e}", themes.display()))?;
            }
        }
    }
    Ok((backup, copied))
}

fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("could not remove {}: {e}", path.display())),
        _ => Ok(()),
    }
}

/// Empties `folder`, keeping the folder itself and every path in `keep` (with the folders that
/// hold one); links are never followed.
fn remove_except(folder: &Path, keep: &[PathBuf]) -> Result<(), String> {
    let entries = std::fs::read_dir(folder).map_err(|e| format!("could not read {}: {e}", folder.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("could not read {}: {e}", folder.display()))?.path();
        if keep.iter().any(|kept| kept == &path) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let removed = if meta.is_dir() && keep.iter().any(|kept| kept.starts_with(&path)) {
            remove_except(&path, keep)?;
            continue;
        } else if meta.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.map_err(|e| format!("could not remove {}: {e}", path.display()))?;
    }
    Ok(())
}

fn copy_file(from: &Path, to: &Path) -> Result<usize, String> {
    let ordinary = std::fs::symlink_metadata(from).is_ok_and(|m| m.is_file());
    if !ordinary || std::fs::symlink_metadata(to).is_ok() {
        return Ok(0);
    }
    std::fs::copy(from, to).map(|_| 1).map_err(|e| format!("could not copy {}: {e}", from.display()))
}

/// Copies the folder `from` into `to`. An entry it cannot read stops the copy, before anything
/// is removed; a link is not followed or copied, and is added to `links` to stay where it is.
fn copy_tree(
    from: &Path,
    to: &Path,
    copied: &mut usize,
    seen: &mut usize,
    links: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if !std::fs::symlink_metadata(from).is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    std::fs::create_dir_all(to).map_err(|e| format!("could not make {}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("could not read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("could not read {}: {e}", from.display()))?;
        *seen += 1;
        if *seen > MAX_ENTRIES {
            return Err(format!("{} holds too many files to copy", from.display()));
        }
        let path = entry.path();
        let target = to.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        if meta.is_dir() {
            copy_tree(&path, &target, copied, seen, links)?;
        } else if meta.is_file() {
            *copied += copy_file(&path, &target)?;
        } else {
            links.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_folder_is_named_copied_into_and_forgotten() {
        let base = std::env::temp_dir().join(format!("suhina-data-folder-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&base);
        let (default, chosen) = (base.join("default"), base.join("chosen"));
        std::fs::create_dir_all(default.join("themes/night")).unwrap();
        std::fs::write(default.join("settings.txt"), "theme = night\n").unwrap();
        std::fs::write(default.join("themes/night/theme.css"), ":root {}").unwrap();
        assert_eq!(pointed(&default), None);
        assert_eq!(copy_over(&default, &chosen), Ok(2));
        assert!(chosen.join("themes/night/theme.css").is_file());
        // What the chosen folder has already is kept.
        std::fs::write(chosen.join("settings.txt"), "theme = day\n").unwrap();
        assert_eq!(copy_over(&default, &chosen), Ok(0));
        assert_eq!(std::fs::read_to_string(chosen.join("settings.txt")).unwrap(), "theme = day\n");
        assert_eq!(point(&default, Some(&chosen)), Ok(()));
        assert_eq!(pointed(&default), Some(chosen.clone()));
        assert_eq!(point(&default, None), Ok(()));
        assert_eq!(pointed(&default), None);
        // Moving: a backup beside the old folder, the files in the new one, none left behind.
        let moved = base.join("moved");
        let (backup, count) = move_data(&default, &moved, false).unwrap();
        assert!(count >= 1 && backup.join("settings.txt").is_file() && moved.join("settings.txt").is_file());
        assert!(!default.join("settings.txt").exists() && !default.join("themes").exists());
        assert!(move_data(&moved, &moved.join("inside"), false).is_err());
        // A folder that is gone is not used.
        point(&default, Some(&base.join("gone"))).unwrap();
        assert_eq!(pointed(&default), None);
        let _clean = std::fs::remove_dir_all(&base);
    }

    /// A theme folder that is a link stays where it is when the folder moves, and what it points
    /// at is never touched; the rest moves.
    #[cfg(unix)]
    #[test]
    fn a_linked_theme_stays_where_it_is_when_the_folder_moves() {
        let base = std::env::temp_dir().join(format!("suhina-data-link-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&base);
        let (themes, elsewhere) = (base.join("themes"), base.join("elsewhere"));
        std::fs::create_dir_all(themes.join("night")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(themes.join("night/theme.css"), ":root {}").unwrap();
        std::fs::write(elsewhere.join("theme.css"), ":root {}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, themes.join("linked")).unwrap();
        let moved = base.join("moved");
        let (backup, _) = move_data(&themes, &moved, true).unwrap();
        assert!(moved.join("night/theme.css").is_file() && backup.join("night/theme.css").is_file());
        assert!(!themes.join("night").exists(), "what was copied leaves");
        assert!(std::fs::symlink_metadata(themes.join("linked")).is_ok_and(|m| m.file_type().is_symlink()));
        assert!(elsewhere.join("theme.css").is_file(), "the linked folder is never touched");
        // The whole data folder: the link stays in its themes folder, which stays for it.
        let data = base.join("data");
        std::fs::create_dir_all(data.join("themes")).unwrap();
        std::fs::write(data.join("settings.txt"), "theme = night\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, data.join("themes/linked")).unwrap();
        move_data(&data, &base.join("data-moved"), false).unwrap();
        assert!(!data.join("settings.txt").exists());
        assert!(std::fs::symlink_metadata(data.join("themes/linked")).is_ok_and(|m| m.file_type().is_symlink()));
        assert!(elsewhere.join("theme.css").is_file());
        let _clean = std::fs::remove_dir_all(&base);
    }
}
