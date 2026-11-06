// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking, unpacking and putting a downloaded package in place of the running build
//! (`docs/spec/updates.md` › Installing in place).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// What a file the update moved aside is renamed to (Windows), deleted on the next start.
const OLD: &str = "old";
/// The folder beside the program a download is unpacked into before it is moved in place.
pub const STAGING: &str = ".suhina-update";

/// The SHA-256 of the file at `path`, in lower-case hex.
pub fn sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("could not read the download: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(|e| format!("could not read the download: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Unpacks the package at `archive` into the empty folder `into`, and returns the folder in it
/// that holds the program (the package's own top folder, or `Suhina.app/Contents/MacOS` in it on
/// macOS). A test build's artifact is a zip around the package: on Linux and macOS around its
/// `.tar.gz`, which is unpacked too.
pub fn unpack(archive: &Path, into: &Path) -> Result<PathBuf, String> {
    let mut magic = [0; 2];
    File::open(archive)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|e| format!("could not read the download: {e}"))?;
    match magic {
        [b'P', b'K'] => unzip(archive, into)?,
        [0x1f, 0x8b] => untar(archive, into)?,
        _ => return Err("the download is not a zip or a .tar.gz".to_string()),
    }
    // A zip holding only a .tar.gz (a test build's artifact on Linux and macOS).
    let entries = list(into)?;
    if let [single] = entries.as_slice()
        && single.extension().is_some_and(|e| e == "gz")
    {
        let inner = single.with_extension("unpacked");
        std::fs::create_dir(&inner).map_err(|e| format!("could not unpack the download: {e}"))?;
        untar(single, &inner)?;
        return program_folder(&inner);
    }
    program_folder(into)
}

fn list(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| format!("could not read {}: {e}", dir.display()))?;
    let mut entries: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    Ok(entries)
}

fn program_name() -> &'static str {
    if cfg!(windows) { "suhina.exe" } else { "suhina" }
}

/// The folder under `dir` (itself or one folder down) that holds the program, or the
/// `Contents/MacOS` folder of an app bundle there.
fn program_folder(dir: &Path) -> Result<PathBuf, String> {
    let mut folders = vec![dir.to_path_buf()];
    folders.extend(app_folders(dir));
    for child in list(dir)?.into_iter().filter(|p| p.is_dir()) {
        folders.extend(app_folders(&child));
        folders.push(child);
    }
    folders
        .into_iter()
        .find(|p| p.join(program_name()).is_file())
        .ok_or_else(|| format!("the download has no {}", program_name()))
}

/// The `Contents/MacOS` folders of the app bundles (`*.app`) in `dir`.
fn app_folders(dir: &Path) -> Vec<PathBuf> {
    let bundles = list(dir).unwrap_or_default().into_iter().filter(|p| p.extension().is_some_and(|e| e == "app"));
    bundles.map(|p| p.join("Contents").join("MacOS")).collect()
}

/// The app bundle (`Suhina.app`) whose `Contents/MacOS` folder `dir` is, when it is one.
pub fn bundle_of(dir: &Path) -> Option<PathBuf> {
    let contents = dir.parent().filter(|_| dir.file_name().is_some_and(|n| n == "MacOS"))?;
    let bundle = contents.parent().filter(|_| contents.file_name().is_some_and(|n| n == "Contents"))?;
    bundle.extension().is_some_and(|e| e == "app").then(|| bundle.to_path_buf())
}

/// Where a download for the program in `dir` is staged: beside its app bundle when it runs from
/// one, so that the bundle can be swapped as a whole, and otherwise beside the program.
pub fn staging_folder(dir: &Path) -> PathBuf {
    let base = bundle_of(dir).and_then(|b| b.parent().map(Path::to_path_buf));
    base.unwrap_or_else(|| dir.to_path_buf()).join(STAGING)
}

/// Puts the app bundle `new` in place of the running one, `old`: the old one is renamed to
/// `Suhina.app.old` (deleted on the next start) and the new one moved in. Swapping the bundle
/// as a whole keeps its signature, which covers every file in it, whole. If the move fails the
/// old bundle is put back.
pub fn replace_bundle(new: &Path, old: &Path) -> Result<(), String> {
    let aside = aside_name(old);
    std::fs::rename(old, &aside).map_err(|e| format!("could not move {} aside: {e}", old.display()))?;
    std::fs::rename(new, old).map_err(|e| {
        let _back = std::fs::rename(&aside, old);
        format!("could not put {} in place: {e}", old.display())
    })
}

/// Puts an unpacked download, `program`, in place of the one running from `dir`: a bundle takes
/// a bundle's place whole, so its signature keeps covering every file in it; a download without
/// an app bundle is refused while the running app is a bundle, for its files cannot go inside
/// the bundle's signature; otherwise the files go in place one by one.
pub fn put_in_place(program: &Path, dir: &Path) -> Result<(), String> {
    match (bundle_of(program), bundle_of(dir)) {
        (Some(new), Some(old)) => replace_bundle(&new, &old),
        (None, Some(_)) => {
            Err("the download has no Suhina.app, and the running app is one: download a build that has the app"
                .to_string())
        }
        _ => replace(program, dir).map(|_| ()),
    }
}

fn unzip(archive: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("could not read the download: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("the download is not a readable zip: {e}"))?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| format!("the zip is broken: {e}"))?;
        // `enclosed_name` refuses names that would land outside `into` (`../`, absolute paths).
        let Some(name) = entry.enclosed_name() else { continue };
        let path = into.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| format!("could not unpack: {e}"))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not unpack: {e}"))?;
        }
        let mut out = File::create(&path).map_err(|e| format!("could not unpack {}: {e}", path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("could not unpack {}: {e}", path.display()))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _kept = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

fn untar(archive: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("could not read the download: {e}"))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_permissions(true);
    // `unpack` refuses entries that would land outside `into`.
    tar.unpack(into).map_err(|e| format!("could not unpack the download: {e}"))
}

/// Moves every file of the unpacked package `from` in place of the one of the same name in
/// `dir`, the running program's folder. On Windows the file there is renamed aside first (a
/// running program's file can be renamed, not overwritten). Any failure puts back what was
/// moved, so the running build is left whole.
pub fn replace(from: &Path, dir: &Path) -> Result<usize, String> {
    let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let result = (|| {
        for source in list(from)? {
            if !source.is_file() {
                continue;
            }
            let Some(name) = source.file_name() else { continue };
            let target = dir.join(name);
            let aside = if cfg!(windows) && target.exists() {
                let aside = aside_name(&target);
                std::fs::rename(&target, &aside)
                    .map_err(|e| format!("could not move {} aside: {e}", target.display()))?;
                Some(aside)
            } else {
                None
            };
            moved.push((target.clone(), aside));
            std::fs::rename(&source, &target)
                .map_err(|e| format!("could not put {} in place: {e}", target.display()))?;
        }
        Ok(moved.len())
    })();
    if result.is_err() {
        for (target, aside) in moved.into_iter().rev() {
            if let Some(aside) = aside {
                let _removed = std::fs::remove_file(&target);
                let _back = std::fs::rename(&aside, &target);
            }
        }
    }
    result
}

/// `suhina.exe.old` (or `Suhina.app.old`), or `suhina.exe.old2` and on while an older one is
/// still there (still running, so it could not be deleted).
fn aside_name(target: &Path) -> PathBuf {
    let base = target.as_os_str().to_owned();
    let mut n = 1;
    loop {
        let mut name = base.clone();
        name.push(if n == 1 { format!(".{OLD}") } else { format!(".{OLD}{n}") });
        let path = PathBuf::from(name);
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if !path.exists() || removed.is_ok() {
            return path;
        }
        n += 1;
    }
}

/// Deletes what an earlier update left in `dir`, the program's folder: files moved aside, the
/// app bundle moved aside (beside the running one), and an unfinished download.
pub fn clean_up(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if is_aside(&path) && path.is_file() {
            let _removed = std::fs::remove_file(&path);
        }
    }
    // Only `<the running bundle's name>.old…`, never another app beside it.
    if let Some(bundle) = bundle_of(dir)
        && let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name())
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.file_stem() == Some(name) && is_aside(&path) && path.is_dir() {
                let _removed = std::fs::remove_dir_all(&path);
            }
        }
    }
    let _removed = std::fs::remove_dir_all(staging_folder(dir));
    let _removed = std::fs::remove_dir_all(dir.join(STAGING));
}

/// Whether `path` was moved aside by an update: its extension is `old`, `old2` and on.
fn is_aside(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.strip_prefix(OLD).is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit())))
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
