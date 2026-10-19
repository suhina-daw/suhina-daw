// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! File access for agents. An MCP client may read only below its read roots and write only below
//! its write root; without a root it has no access of that kind. `suhina-cli mcp` gets one read
//! root and one write root at launch; an agent attached to the app (ADR 0028) gets the folder of
//! the project it acts on to write and read, and the browser's library folders to read.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    /// The first is where relative paths start.
    read_roots: Vec<PathBuf>,
    write_root: Option<PathBuf>,
    /// What to say when a root is missing: how this kind of server gets one.
    missing_read: &'static str,
    missing_write: &'static str,
}

impl Workspace {
    /// Both roots must exist; they are resolved to absolute paths now, so later checks compare
    /// real locations.
    pub fn new(read_root: Option<&Path>, write_root: Option<&Path>) -> Result<Self, String> {
        let resolve = |root: Option<&Path>, what: &str| -> Result<Option<PathBuf>, String> {
            root.map(|r| r.canonicalize().map_err(|e| format!("{what} root {}: {e}", r.display()))).transpose()
        };
        Ok(Self {
            read_roots: resolve(read_root, "read")?.into_iter().collect(),
            write_root: resolve(write_root, "write")?,
            missing_read: "no read root was granted (start the server with --read-root)",
            missing_write: "no write root was granted (start the server with --write-root)",
        })
    }

    /// An attached agent's roots (ADR 0028): `project_folder`, the folder of the project it acts
    /// on, to write and read (`None` for a project never saved), and `library` to read. Folders
    /// that do not exist are left out.
    pub fn attached(project_folder: Option<&Path>, library: &[PathBuf]) -> Self {
        let project = project_folder.and_then(|f| f.canonicalize().ok());
        let mut read_roots: Vec<PathBuf> = project.iter().cloned().collect();
        for folder in library {
            if let Ok(real) = folder.canonicalize()
                && !read_roots.contains(&real)
            {
                read_roots.push(real);
            }
        }
        Self {
            read_roots,
            write_root: project,
            missing_read: "this project has no folder yet and the browser has none: save the project first",
            missing_write: "this project has no folder yet: the person saves it once before an agent can write",
        }
    }

    /// An existing file below a read root. `path` is relative to the first root, or absolute and
    /// inside one; symlinks that lead outside are refused.
    pub fn readable(&self, path: &str) -> Result<PathBuf, String> {
        let real = self.read_real(path)?;
        if !real.is_file() {
            return Err(format!("{path} is not a file"));
        }
        Ok(real)
    }

    /// An existing folder below a read root, resolved as [`Workspace::readable`] resolves
    /// files.
    pub fn readable_folder(&self, path: &str) -> Result<PathBuf, String> {
        let real = self.read_real(path)?;
        if !real.is_dir() {
            return Err(format!("{path} is not a folder"));
        }
        Ok(real)
    }

    /// `path` made real, inside one of the read roots.
    fn read_real(&self, path: &str) -> Result<PathBuf, String> {
        let first = self.read_roots.first().ok_or(self.missing_read)?;
        let candidate = join(first, path)?;
        let real = candidate.canonicalize().map_err(|e| format!("{path}: {e}"))?;
        if !self.read_roots.iter().any(|root| real.starts_with(root)) {
            return Err(format!("{path} is outside the read root"));
        }
        Ok(real)
    }

    /// A file to create below the write root. Its folder must exist inside the root; an existing
    /// file is replaced only when `overwrite` is set, and never through a symlink.
    pub fn writable(&self, path: &str, overwrite: bool) -> Result<PathBuf, String> {
        let root = self.write_root.as_ref().ok_or(self.missing_write)?;
        let candidate = join(root, path)?;
        let name = candidate.file_name().ok_or_else(|| format!("{path} has no file name"))?.to_owned();
        let parent = candidate.parent().ok_or_else(|| format!("{path} has no folder"))?;
        let real_parent = parent.canonicalize().map_err(|e| format!("{path}: folder: {e}"))?;
        if !real_parent.starts_with(root) {
            return Err(format!("{path} is outside the write root"));
        }
        let target = real_parent.join(name);
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() => {
                Err(format!("{path} is a symlink; refusing to write through it"))
            }
            Ok(meta) if meta.is_dir() => Err(format!("{path} is a folder")),
            Ok(_) if !overwrite => Err(format!("{path} already exists; pass overwrite: true to replace it")),
            _ => Ok(target),
        }
    }

    /// An existing folder below the write root, resolved as [`Workspace::readable`] resolves
    /// files, for something to write into it.
    pub fn writable_folder(&self, path: &Path) -> Result<PathBuf, String> {
        let root = self.write_root.as_ref().ok_or(self.missing_write)?;
        let real = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
        if !real.starts_with(root) || !real.is_dir() {
            return Err(format!("{} is not a folder below the write root", path.display()));
        }
        Ok(real)
    }
}

/// A path for people to read: Windows' verbatim prefix (`\\?\`), which canonicalising adds, is
/// dropped.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}

/// Joins a relative `path` to `root`, refusing `..`. An absolute path is taken as it is: the
/// callers resolve it (or its folder) and check that the real location is inside a root, which
/// a text comparison cannot do on Windows, where a canonical root has the verbatim prefix and a
/// given path may name a folder by its short name.
fn join(root: &Path, path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    if path.is_empty() {
        return Err("path is empty".into());
    }
    if given.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{path}: '..' is not allowed"));
    }
    if given.is_absolute() {
        return Ok(given.to_path_buf());
    }
    Ok(root.join(given))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-workspace-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        std::fs::write(dir.join("inside").join("a.als"), b"x").unwrap();
        std::fs::write(dir.join("outside.als"), b"x").unwrap();
        dir
    }

    #[test]
    fn display_path_drops_the_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\a\b.wav")), r"C:\a\b.wav");
        assert_eq!(display_path(Path::new("/tmp/b.wav")), "/tmp/b.wav");
    }

    #[test]
    fn reads_only_below_the_read_root() {
        let dir = temp_dir("read");
        let ws = Workspace::new(Some(&dir.join("inside")), None).unwrap();
        assert!(ws.readable("a.als").is_ok());
        assert!(ws.readable("../outside.als").is_err());
        assert!(ws.readable(dir.join("outside.als").to_str().unwrap()).is_err());
        assert!(ws.readable("missing.als").is_err());
        assert!(ws.readable("").is_err());
        assert!(ws.writable("x.wav", false).is_err(), "no write root granted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_only_below_the_write_root_and_never_overwrites_by_accident() {
        let dir = temp_dir("write");
        let ws = Workspace::new(None, Some(&dir.join("inside"))).unwrap();
        assert!(ws.writable("new.wav", false).is_ok());
        assert!(ws.writable("a.als", false).is_err());
        assert!(ws.writable("a.als", true).is_ok());
        assert!(ws.writable("../escape.wav", false).is_err());
        assert!(ws.writable("missing-folder/x.wav", false).is_err());
        assert!(ws.readable("a.als").is_err(), "no read root granted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
