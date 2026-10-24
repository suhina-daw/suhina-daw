// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A theme package (`.theme.suhina`): one theme's folder in a zip, with `manifest.json` at its
//! root naming the theme and every file's SHA-256 sum. Packing reads a folder; reading checks the
//! paths, the sizes, the sums and what each file holds before anything is written, so the app,
//! `suhina-cli theme validate` and an import all refuse the same packages
//! (`docs/spec/theme-skins.md` › Theme packages).
//!
//! A package is not signed: whoever shares one signs the file with a key of their own, outside
//! Suhina, and the receiver checks that signature before opening it.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

/// The file name ending of a package.
pub const EXTENSION: &str = ".theme.suhina";
/// The manifest's name at the package's root.
pub const MANIFEST: &str = "manifest.json";
/// What the manifest's `format` says.
const FORMAT: &str = "suhina-theme";
/// The manifest version this build writes and reads.
const VERSION: u64 = 1;

/// A style sheet or an SVG is at most this big, as the app reads them.
pub const MAX_TEXT: usize = 128 * 1024;
/// A PNG is at most this big.
pub const MAX_PNG: usize = 8 * 1024 * 1024;
/// Any other file (a variant, a README) is at most this big.
const MAX_OTHER: usize = 1024 * 1024;
/// All files together, unpacked.
pub const MAX_TOTAL: usize = 32 * 1024 * 1024;
/// Files in one package.
pub const MAX_FILES: usize = 512;

/// A package read and checked: its theme's id and name, its files by path, and what the checks
/// found that does not stop it from being used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub files: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
}

/// Whether `path` names a package by its ending.
pub fn is_package(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().ends_with(EXTENSION))
}

/// A relative path inside a theme: forward slashes, no `.` or `..` parts, no drive or device.
pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The SHA-256 sum of `bytes` as lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks a theme's files: a `theme.css` that reads, every file within its size, SVGs that are
/// SVG and PNGs that are PNG, variants that are JSON. Returns the warnings (a sheet's notices),
/// or the first error.
pub fn validate(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<String>, String> {
    if files.len() > MAX_FILES {
        return Err(format!("a theme has at most {MAX_FILES} files"));
    }
    let mut total = 0usize;
    let mut warnings = Vec::new();
    for (path, bytes) in files {
        if !safe_relative(path) {
            return Err(format!("{path}: not a path inside the theme"));
        }
        total = total.checked_add(bytes.len()).ok_or("the theme is too big")?;
        if total > MAX_TOTAL {
            return Err(format!("the theme's files exceed {} MiB together", MAX_TOTAL / 1024 / 1024));
        }
        let lower = path.to_ascii_lowercase();
        let limit = if lower.ends_with(".css") || lower.ends_with(".svg") {
            MAX_TEXT
        } else if lower.ends_with(".png") {
            MAX_PNG
        } else {
            MAX_OTHER
        };
        if bytes.len() > limit {
            return Err(format!("{path}: exceeds {} KiB", limit / 1024));
        }
        if lower.ends_with(".css") {
            let text = std::str::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8 text"))?;
            let sheet = suhina_style::sheet::Sheet::parse(text).map_err(|e| format!("{path}: {e}"))?;
            warnings
                .extend(sheet.notices.iter().map(|n| format!("{path} line {}:{}: {}", n.line, n.column, n.message)));
        } else if lower.ends_with(".svg") {
            let text = std::str::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8 text"))?;
            let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 100_000, ..Default::default() };
            let doc = roxmltree::Document::parse_with_options(text, options).map_err(|e| format!("{path}: {e}"))?;
            if doc.root_element().tag_name().name() != "svg" {
                return Err(format!("{path}: not an SVG image"));
            }
            let lower = text.to_ascii_lowercase();
            if ["<script", "<image", "<foreignobject", "href="].iter().any(|s| lower.contains(s)) {
                return Err(format!("{path}: an SVG holds only self-contained vector artwork"));
            }
        } else if lower.ends_with(".png") {
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(format!("{path}: not a PNG image"));
            }
        } else if lower.ends_with(".json") {
            serde_json::from_slice::<serde_json::Value>(bytes).map_err(|e| format!("{path}: {e}"))?;
        }
    }
    if !files.contains_key("theme.css") {
        return Err("a theme needs a theme.css at its root".into());
    }
    Ok(warnings)
}

/// Packs a theme's files into a package for the theme `id` named `name`, after the same checks
/// a reader makes.
pub fn pack(id: &str, name: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    if !valid_id(id) {
        return Err(format!("{id}: a theme id is lowercase letters, digits, hyphens and underscores"));
    }
    if files.contains_key(MANIFEST) {
        return Err(format!("a theme's own file may not be named {MANIFEST}"));
    }
    validate(files)?;
    let sums: serde_json::Map<String, serde_json::Value> =
        files.iter().map(|(path, bytes)| (path.clone(), serde_json::Value::String(sha256(bytes)))).collect();
    let manifest = serde_json::json!({ "format": FORMAT, "version": VERSION, "id": id, "name": name, "files": sums });
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let failed = |e: &dyn std::fmt::Display| format!("could not write the package: {e}");
    zip.start_file(MANIFEST, options).map_err(|e| failed(&e))?;
    zip.write_all(&manifest).map_err(|e| failed(&e))?;
    for (path, bytes) in files {
        zip.start_file(path.as_str(), options).map_err(|e| failed(&e))?;
        zip.write_all(bytes).map_err(|e| failed(&e))?;
    }
    Ok(zip.finish().map_err(|e| failed(&e))?.into_inner())
}

/// Reads and checks a package: its manifest, then every file against its sum and the checks of
/// [`validate`]. A file the manifest does not name, or a sum that differs, refuses the package.
pub fn read(bytes: &[u8]) -> Result<Package, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a package: {e}"))?;
    if archive.len() > MAX_FILES + 1 {
        return Err(format!("a theme has at most {MAX_FILES} files"));
    }
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if !safe_relative(&name) || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(format!("{name}: not a path inside the theme"));
        }
        let limit = MAX_PNG.max(MAX_TEXT);
        let mut data = Vec::new();
        (&mut entry).take(limit as u64 + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() > limit {
            return Err(format!("{name}: too big"));
        }
        total = total.saturating_add(data.len());
        if total > MAX_TOTAL + MAX_OTHER {
            return Err("the package's files are too big together".into());
        }
        if entries.insert(name.clone(), data).is_some() {
            return Err(format!("{name}: twice in the package"));
        }
    }
    let manifest = entries.remove(MANIFEST).ok_or("not a theme package: it has no manifest.json")?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).map_err(|e| format!("{MANIFEST}: {e}"))?;
    if manifest["format"] != FORMAT {
        return Err(format!("{MANIFEST}: not a Suhina theme"));
    }
    match manifest["version"].as_u64() {
        Some(VERSION) => {}
        Some(newer) if newer > VERSION => {
            return Err(format!("the package is version {newer}; this Suhina reads version {VERSION}: update it"));
        }
        _ => return Err(format!("{MANIFEST}: no version")),
    }
    let id = manifest["id"].as_str().filter(|id| valid_id(id)).ok_or(format!("{MANIFEST}: no valid id"))?.to_owned();
    let name = manifest["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&id).to_owned();
    let sums = manifest["files"].as_object().ok_or(format!("{MANIFEST}: no list of files"))?;
    for (path, bytes) in &entries {
        let sum = sums.get(path).and_then(serde_json::Value::as_str).ok_or(format!("{path}: not in the manifest"))?;
        if !sum.eq_ignore_ascii_case(&sha256(bytes)) {
            return Err(format!(
                "{path}: its SHA-256 sum differs from the manifest's: the package is damaged or changed"
            ));
        }
    }
    if let Some(missing) = sums.keys().find(|path| !entries.contains_key(*path)) {
        return Err(format!("{missing}: named in the manifest but not in the package"));
    }
    let warnings = validate(&entries)?;
    Ok(Package { id, name, files: entries, warnings })
}

/// A theme folder's files by path, without hidden files and folders (`.git`, `.DS_Store`).
pub fn read_folder(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        let entries = std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err(format!("{path}: a link; a theme holds only its own files"));
            }
            if kind.is_dir() {
                if prefix.matches('/').count() >= 8 {
                    return Err(format!("{path}: folders nested too deep"));
                }
                pending.push((entry.path(), path));
                continue;
            }
            if files.len() >= MAX_FILES {
                return Err(format!("a theme has at most {MAX_FILES} files"));
            }
            let file = std::fs::File::open(entry.path()).map_err(|e| format!("{path}: {e}"))?;
            let mut bytes = Vec::new();
            file.take(MAX_PNG as u64 + 1).read_to_end(&mut bytes).map_err(|e| format!("{path}: {e}"))?;
            total = total.saturating_add(bytes.len());
            if bytes.len() > MAX_PNG || total > MAX_TOTAL {
                return Err(format!("{path}: the theme is too big"));
            }
            files.insert(path, bytes);
        }
    }
    Ok(files)
}

/// The theme's name from its `theme.css` (`theme { name: "…" }`), when it says one.
pub fn sheet_name(css: &str) -> Option<String> {
    let start = css.find("theme")?;
    let body = &css[start..];
    let open = body.find('{')?;
    let close = body[open..].find('}')? + open;
    body[open + 1..close].split(';').find_map(|declaration| {
        let (property, value) = declaration.split_once(':')?;
        (property.trim() == "name").then(|| value.trim().trim_matches(['"', '\'']).to_owned()).filter(|n| !n.is_empty())
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| (c.is_alphanumeric() && !c.is_uppercase()) || c == '-' || c == '_')
}

#[cfg(test)]
mod tests;
