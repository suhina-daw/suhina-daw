// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped surface programs (docs/spec/shipped-scripts.md, #1676): the scripts Suhina
//! carries inside the executable, unpacks into the library's `Scripts/MIDI/` folder at start —
//! the pack's own rule ([ADR 0036] › 3): each file written only when it is not there, a file
//! the user carries never overwritten, never moved — and installs into the extensions store,
//! where the store's port scan runs them ([`install`]). A script that could not be written or
//! installed is said in the start-up warnings while the next start tries again.

use std::collections::BTreeMap;
use std::path::Path;

use crate::extensions::{Extensions, Manifest};

/// One shipped script: its manifest's `id` — the folder it unpacks into under `Scripts/MIDI/` —
/// and its files as `(name, bytes)`, embedded from the tree's `packs/<id>/` ([ADR 0036] › 3).
/// Beside the script ride its manifest and a README carrying where the script's knowledge
/// comes from and its licence ([ADR 0036] › 1 and 6).
pub struct ShippedScript {
    pub id: &'static str,
    pub files: &'static [(&'static str, &'static [u8])],
}

/// Every shipped script, in the order the unpack walks them. A test keeps this list and the
/// `packs/<id>/` folders the same, so a file added to a pack is never forgotten here.
pub fn scripts() -> &'static [ShippedScript] {
    static SCRIPTS: &[ShippedScript] = &[ShippedScript {
        id: "apc40-mkii",
        files: &[
            ("extension.json", include_bytes!("../../../packs/apc40-mkii/extension.json")),
            ("script.lua", include_bytes!("../../../packs/apc40-mkii/script.lua")),
            ("README.md", include_bytes!("../../../packs/apc40-mkii/README.md")),
        ],
    }];
    SCRIPTS
}

/// Unpacks every shipped script into `scripts_root` — the library's `Scripts/MIDI/` folder
/// (`Documents/Suhina/Scripts/MIDI`, [ADR 0036] › 2) — each file only when it is missing: a file
/// the user carries is theirs, never overwritten, never moved. Returns how many files were
/// written and the lines a script that could not be written should say, one per script — the
/// next start tries again. `SUHINA_PACK_DEBUG=1` prints the walk to stderr, like the NAM
/// pack's unpack.
pub fn unpack(scripts_root: &Path) -> (usize, Vec<String>) {
    let debug = std::env::var("SUHINA_PACK_DEBUG").as_deref() == Ok("1");
    let mut written = 0;
    let mut warnings = Vec::new();
    for script in scripts() {
        match unpack_one(scripts_root, script) {
            Ok(count) => {
                if debug {
                    eprintln!("shipped scripts: {} wrote {count} file(s)", script.id);
                }
                written += count;
            }
            Err(e) => {
                if debug {
                    eprintln!("shipped scripts: {} did not unpack: {e}", script.id);
                }
                warnings.push(format!(
                    "the shipped {} script could not be written into {}: {e}; the next start tries again",
                    script.id,
                    scripts_root.join(script.id).display()
                ));
            }
        }
    }
    if debug {
        eprintln!("shipped scripts: {written} file(s) written");
    }
    (written, warnings)
}

/// One script's files into its folder under `scripts_root`, each only when it is missing. The
/// first file that cannot be written stops the script with its error: the folder is one
/// write, and one line about it is enough. Files already there are not touched and not
/// counted.
fn unpack_one(scripts_root: &Path, script: &ShippedScript) -> std::io::Result<usize> {
    let folder = scripts_root.join(script.id);
    let mut written = 0;
    for &(name, bytes) in script.files {
        let target = folder.join(name);
        if target.exists() {
            continue;
        }
        std::fs::create_dir_all(&folder)?;
        if publish(&target, bytes)? {
            written += 1;
        }
    }
    Ok(written)
}

/// Writes `bytes` beside `target` first, then publishes the completed file — a write that
/// fails (or an app that ends mid-write) leaves no partial file at the target, and an existing
/// file is never replaced: `false` for one that appeared while this ran, another start's.
fn publish(target: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    let staging =
        target.with_file_name(format!("{}.suhina-new", target.file_name().unwrap_or_default().to_string_lossy()));
    if let Err(e) = std::fs::write(&staging, bytes) {
        let _ = std::fs::remove_file(&staging);
        return Err(e);
    }
    // A link never replaces: a file another writer made since the caller's look is not touched,
    // and the staging copy goes. A filesystem without hard links falls back to creating the
    // target exclusively, so an existing file is still never replaced; a write that fails
    // midway leaves a partial file only where no file existed, and later starts leave it alone.
    match std::fs::hard_link(&staging, target) {
        Ok(()) => {
            let _ = std::fs::remove_file(&staging);
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&staging);
            Ok(false)
        }
        Err(_) => {
            use std::io::Write;
            let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(target) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let _ = std::fs::remove_file(&staging);
                    return Ok(false);
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&staging);
                    return Err(e);
                }
            };
            let written = file.write_all(bytes);
            drop(file);
            let _ = std::fs::remove_file(&staging);
            written.map(|()| true)
        }
    }
}

/// The record's name in the store's root: a dotfile beside the extension folders, and
/// [`Extensions::reload`] reads folders only — it is never listed as an extension itself.
const RECORD_FILE: &str = ".shipped-scripts.json";

/// The record format this reads and writes; a record from a newer Suhina reads as empty
/// rather than as fields this one does not know.
const RECORD_VERSION: u32 = 1;

/// The most the record may hold: a corrupted or hostile file is not read whole.
const RECORD_MAX_BYTES: u64 = 256 * 1024;

/// The most of a store file the install reads to compare it: past it the copy is not one
/// Suhina installed — the script's own cap, the largest file a shipped script carries.
const COMPARE_MAX_BYTES: usize = suhina_plugins::surface::MAX_MESSAGE_BYTES;

/// What Suhina installed into the store, in the store's own record: per script id, the files
/// and their SHA-256, and whether the user removed it afterwards.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Record {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    scripts: BTreeMap<String, RecordEntry>,
}

/// One installed script in the [`Record`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct RecordEntry {
    /// The installed files, name to SHA-256.
    #[serde(default)]
    files: BTreeMap<String, String>,
    /// The user ran `extensions.remove` after Suhina installed it: later starts do not
    /// install it again ([`crate::extensions`]'s remove marks it, [`note_removed`]).
    #[serde(default)]
    removed: bool,
}

impl Record {
    /// The store's record; a missing, unreadable or malformed one reads as empty — it is
    /// Suhina's own state, and a corrupted one must not stop the start.
    fn read(root: &Path) -> Self {
        use std::io::Read;
        let path = root.join(RECORD_FILE);
        let Ok(file) = std::fs::File::open(&path) else { return Self::default() };
        let mut bytes = Vec::new();
        // The cap is read as the file is read, not from a size taken before it: a record that
        // grows between the two calls is still never loaded whole (#1720 review).
        match file.take(RECORD_MAX_BYTES + 1).read_to_end(&mut bytes) {
            Ok(read) if read as u64 <= RECORD_MAX_BYTES => {}
            _ => return Self::default(),
        }
        let Ok(record) = serde_json::from_slice::<Self>(&bytes) else { return Self::default() };
        if record.version > RECORD_VERSION {
            return Self::default();
        }
        record
    }

    /// Writes the record beside itself first, then renames it into place: a failed or
    /// interrupted write leaves the old record, not half of one.
    fn write(&self, root: &Path) -> std::io::Result<()> {
        let record = Record { version: RECORD_VERSION, scripts: self.scripts.clone() };
        let Ok(bytes) = serde_json::to_vec(&record) else {
            return Err(std::io::Error::other("the record cannot be written"));
        };
        let target = root.join(RECORD_FILE);
        let staging = root.join(format!("{RECORD_FILE}.suhina-new"));
        crate::files::stage_and_rename(&staging, &target, &bytes)
    }
}

/// Installs every shipped script into the extensions store, the way `extensions.add` copies a
/// folder in, and keeps the copies Suhina installed up to date (docs/spec/shipped-scripts.md
/// › Where it lands, and what runs):
///
/// - The store has never held it and no folder of its own carries the id: the shipped files
///   install from the executable.
/// - The record says Suhina installed it and the folder is gone: the user removed it, or an
///   update was interrupted — a removal stays removed ([`note_removed`]), an interrupted
///   install is tried again.
/// - The store's copy still carries exactly the files the record names, and the app carries a
///   newer shipped version: replaced whole, and the record follows.
/// - The store's copy differs from the record — the user's edits, their own extension under
///   the id, or their copy added back from the library's folder: theirs, never overwritten.
///
/// Returns how many scripts were installed or replaced and the lines a start-up failure
/// should say; the next start tries again. `SUHINA_PACK_DEBUG=1` prints the walk, like the
/// NAM pack's unpack.
pub fn install(extensions: &mut Extensions) -> (usize, Vec<String>) {
    let debug = std::env::var("SUHINA_PACK_DEBUG").as_deref() == Ok("1");
    let Some(root) = extensions.root().map(Path::to_path_buf) else {
        if debug {
            eprintln!("shipped scripts: no extensions store is set");
        }
        return (0, Vec::new());
    };
    let mut record = Record::read(&root);
    let mut warnings = Vec::new();
    let mut installed = 0;
    let mut published: Vec<String> = Vec::new();
    for script in scripts() {
        let folder = root.join(script.id);
        let shipped: BTreeMap<String, String> =
            script.files.iter().map(|&(name, bytes)| (name.to_string(), digest(bytes))).collect();
        match record.scripts.get(script.id) {
            // One with the id is already there — the user's own extension, or their copy
            // added from the library's folder: theirs, and not Suhina's to manage.
            None if folder.exists() => {
                if debug {
                    eprintln!("shipped scripts: {} is already in the store, left alone", script.id);
                }
                continue;
            }
            Some(entry) if entry.removed && !folder.exists() => {
                if debug {
                    eprintln!("shipped scripts: {} was removed; it stays removed", script.id);
                }
                continue;
            }
            // Added back from the library after its removal: the user's copy now, even when its
            // bytes match what Suhina shipped, and a later version does not replace it
            // (docs/spec/shipped-scripts.md › Where it lands, and what runs).
            Some(entry) if entry.removed && folder.exists() => {
                if debug {
                    eprintln!("shipped scripts: {} was added back; it is the user's now", script.id);
                }
                continue;
            }
            Some(entry) if folder.exists() && !matches_installed(&folder, &entry.files) => {
                if debug {
                    eprintln!("shipped scripts: {} in the store is not the installed copy, left alone", script.id);
                }
                continue;
            }
            Some(entry) if folder.exists() && entry.files == shipped => {
                if debug {
                    eprintln!("shipped scripts: {} is in the store and current", script.id);
                }
                continue;
            }
            _ => {}
        }
        // Only an untouched copy of ours can be here with the folder already there: a fresh
        // install or a replacement of that copy.
        let replace = folder.exists();
        match install_one(&root, script, replace) {
            Ok(()) => {
                if debug {
                    eprintln!("shipped scripts: {} {}", script.id, if replace { "replaced" } else { "installed" });
                }
                record.scripts.insert(script.id.to_string(), RecordEntry { files: shipped, removed: false });
                installed += 1;
                published.push(script.id.to_string());
            }
            Err(reason) => {
                if debug {
                    eprintln!("shipped scripts: {} did not install: {reason}", script.id);
                }
                warnings.push(format!(
                    "the shipped {} script could not be installed into the extensions store: {reason}; the next start tries again",
                    script.id
                ));
            }
        }
    }
    if installed > 0 {
        if let Err(e) = record.write(&root) {
            // The record did not land: a fresh copy would look like the user's own, and a
            // replaced one would look changed, so later starts would skip it. Take the copies
            // back out, so the next start sees no folder (a fresh install) or a missing one
            // (an update) and installs again.
            let mut left = Vec::new();
            for id in &published {
                if std::fs::remove_dir_all(root.join(id)).is_err() {
                    left.push(id.as_str());
                }
            }
            let followed = if left.is_empty() {
                "the next start installs again".to_string()
            } else {
                format!("these copies are still in the store and are left alone: {}", left.join(", "))
            };
            warnings.push(format!(
                "the shipped scripts' record could not be written into {}: {e}; {followed}",
                root.display()
            ));
        }
        extensions.reload();
    }
    (installed, warnings)
}

/// Writes `script`'s files into the store under its id: a staging folder in the root,
/// validated with the store's own manifest reader, published by one rename — a fresh folder,
/// or over the untouched copy an update replaces, which goes first. The publish is the
/// rename, so a failure leaves the old copy (or the clear) and the next start tries again.
fn install_one(root: &Path, script: &ShippedScript, replace: bool) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| format!("the store folder cannot be made: {e}"))?;
    let folder = root.join(script.id);
    let staging = root.join(format!(".shipped-{}.staging", script.id));
    let _ = std::fs::remove_dir_all(&staging);
    let written = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&staging)?;
        for &(name, bytes) in script.files {
            std::fs::write(staging.join(name), bytes)?;
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("its files cannot be written: {e}"));
    }
    if let Err(reason) = Manifest::read(&staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("the staged copy does not pass the store's checks: {reason}"));
    }
    if replace {
        // The old copy goes aside whole, so a failed replacement leaves the complete old one,
        // not a half-deleted folder the record no longer matches (#1720 review).
        let backup = root.join(format!(".shipped-{}.old", script.id));
        let _ = std::fs::remove_dir_all(&backup);
        if let Err(e) = std::fs::rename(&folder, &backup) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("the copy being replaced cannot be set aside: {e}"));
        }
        if let Err(e) = std::fs::rename(&staging, &folder) {
            let _ = std::fs::rename(&backup, &folder);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("the copy cannot be put in the store: {e}"));
        }
        let _ = std::fs::remove_dir_all(&backup);
        return Ok(());
    }
    if let Err(e) = std::fs::rename(&staging, &folder) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("the copy cannot be put in the store: {e}"));
    }
    Ok(())
}

/// Whether the store's folder still carries exactly the files the record names: each listed
/// file is there, is a file, reads within the compare cap, and hashes to its recorded
/// SHA-256, and no other visible entry is there. A hidden entry — the Finder's own notes
/// among them — is not a change; a missing, changed or extra file is, and the copy is the
/// user's from then on.
fn matches_installed(folder: &Path, files: &BTreeMap<String, String>) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else { return false };
    let mut seen = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Some(recorded) = files.get(&name) else { return false };
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            return false;
        }
        let Ok(hash) = crate::extensions::file_digest(&entry.path(), COMPARE_MAX_BYTES) else { return false };
        if &hash != recorded {
            return false;
        }
        seen += 1;
    }
    seen == files.len()
}

/// A SHA-256 as the record writes it: the lowercase hex
/// [`crate::extensions::file_digest`] reads a store file to.
fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Records that the user removed `id` through `extensions.remove`: a removed shipped script
/// is not installed again by later starts ([`install`], docs/spec/shipped-scripts.md › Where
/// it lands, and what runs). Only the ids Suhina ships are marked.
pub(crate) fn note_removed(root: &Path, id: &str) -> std::io::Result<()> {
    if !scripts().iter().any(|script| script.id == id) {
        return Ok(());
    }
    let mut record = Record::read(root);
    let entry = record.scripts.entry(id.to_string()).or_default();
    if entry.removed {
        return Ok(());
    }
    entry.removed = true;
    record.write(root)
}

#[cfg(test)]
#[path = "shipped_scripts_tests.rs"]
mod tests;
