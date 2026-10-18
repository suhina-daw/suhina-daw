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
/// that holds the program (the package's own top folder). A test build's artifact is a zip
/// around the package: on Linux and macOS around its `.tar.gz`, which is unpacked too.
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
    Ok(read.filter_map(Result::ok).map(|e| e.path()).collect())
}

fn program_name() -> &'static str {
    if cfg!(windows) { "suhina.exe" } else { "suhina" }
}

/// The folder under `dir` (itself or one folder down) that holds the program.
fn program_folder(dir: &Path) -> Result<PathBuf, String> {
    if dir.join(program_name()).is_file() {
        return Ok(dir.to_path_buf());
    }
    list(dir)?
        .into_iter()
        .find(|p| p.join(program_name()).is_file())
        .ok_or_else(|| format!("the download has no {}", program_name()))
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

/// `suhina.exe.old`, or `suhina.exe.old2` and on while an older one is still there (still
/// running, so it could not be deleted).
fn aside_name(target: &Path) -> PathBuf {
    let base = target.as_os_str().to_owned();
    let mut n = 1;
    loop {
        let mut name = base.clone();
        name.push(if n == 1 { format!(".{OLD}") } else { format!(".{OLD}{n}") });
        let path = PathBuf::from(name);
        if !path.exists() || std::fs::remove_file(&path).is_ok() {
            return path;
        }
        n += 1;
    }
}

/// Deletes what an earlier update left in `dir`: files moved aside, and an unfinished download.
pub fn clean_up(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let old = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.strip_prefix(OLD).is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit())));
        if old && path.is_file() {
            let _removed = std::fs::remove_file(&path);
        }
    }
    let _removed = std::fs::remove_dir_all(dir.join(STAGING));
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-update-{name}-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        dir
    }

    /// A package as the release workflow makes it: one top folder with the program in it.
    fn package_zip(path: &Path, top: &str, program: &[u8]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(format!("{top}/{}", program_name()), options).unwrap();
        zip.write_all(program).unwrap();
        zip.start_file(format!("{top}/LICENSE"), options).unwrap();
        zip.write_all(b"GPL").unwrap();
        zip.finish().unwrap();
    }

    fn package_tar(path: &Path, top: &str, program: &[u8]) {
        let gz = flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(program.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, format!("{top}/{}", program_name()), program).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn the_checksum_is_the_files_sha256() {
        let dir = folder("sha");
        let file = dir.join("abc");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(sha256(&file).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_package_unpacks_to_the_folder_with_the_program() {
        let dir = folder("unpack");
        let archive = dir.join("package.zip");
        package_zip(&archive, "suhina-nightly-windows-x86_64", b"new");
        let into = dir.join("zip");
        std::fs::create_dir(&into).unwrap();
        let found = unpack(&archive, &into).unwrap();
        assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"new");

        let archive = dir.join("package.tar.gz");
        package_tar(&archive, "suhina-nightly-linux-x86_64", b"newer");
        let into = dir.join("tar");
        std::fs::create_dir(&into).unwrap();
        let found = unpack(&archive, &into).unwrap();
        assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"newer");

        // A test build's artifact: a zip around the .tar.gz.
        let artifact = dir.join("artifact.zip");
        let mut zip = zip::ZipWriter::new(File::create(&artifact).unwrap());
        zip.start_file("suhina-pr663-linux-x86_64.tar.gz", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&std::fs::read(&archive).unwrap()).unwrap();
        zip.finish().unwrap();
        let into = dir.join("artifact");
        std::fs::create_dir(&into).unwrap();
        let found = unpack(&artifact, &into).unwrap();
        assert_eq!(std::fs::read(found.join(program_name())).unwrap(), b"newer");

        std::fs::write(dir.join("page.html"), b"<html>").unwrap();
        assert!(unpack(&dir.join("page.html"), &dir).is_err());
    }

    #[test]
    fn replacing_puts_each_file_in_place_and_cleaning_up_removes_what_was_moved_aside() {
        let dir = folder("replace");
        let program = dir.join("program");
        std::fs::create_dir(&program).unwrap();
        std::fs::write(program.join(program_name()), b"old").unwrap();
        std::fs::write(program.join("settings-of-its-own.txt"), b"kept").unwrap();
        let new = dir.join("new");
        std::fs::create_dir(&new).unwrap();
        std::fs::write(new.join(program_name()), b"new").unwrap();
        std::fs::write(new.join("LICENSE"), b"GPL").unwrap();

        assert_eq!(replace(&new, &program).unwrap(), 2);
        assert_eq!(std::fs::read(program.join(program_name())).unwrap(), b"new");
        assert_eq!(std::fs::read(program.join("LICENSE")).unwrap(), b"GPL");
        assert_eq!(std::fs::read(program.join("settings-of-its-own.txt")).unwrap(), b"kept");
        std::fs::create_dir(program.join(STAGING)).unwrap();
        clean_up(&program);
        let mut left: Vec<String> =
            list(&program).unwrap().iter().filter_map(|p| p.file_name()?.to_str().map(String::from)).collect();
        left.sort();
        let mut expected = vec!["LICENSE".to_string(), program_name().to_string(), "settings-of-its-own.txt".into()];
        expected.sort();
        assert_eq!(left, expected);
    }
}
