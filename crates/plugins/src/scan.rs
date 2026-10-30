// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding installed plug-ins (ADR 0006, decision 9).
//!
//! [`standard_folders`] lists where each system keeps plug-ins; users can add folders of their
//! own. [`Catalog::scan`] looks through the folders for plug-in files and reads each new or
//! changed one in a `suhina-plugin-host --scan` process of its own, with a timeout, so a
//! plug-in that crashes or hangs while loading costs its own entry, nothing else. The catalog
//! keeps what it learned, keyed by path, modification time and size, so the next scan reads only
//! what changed. A failure may be passing (a plug-in that waits on another process of its vendor
//! while loading), so files that fail are read once more, one at a time, at the end of the same
//! scan, and later scans try them again ([`Catalog::scan`]). Two results are settled until the
//! file changes: a file that loads and holds no plug-ins ([`Scanned::NoPlugins`]), and one built
//! only for another processor ([`Scanned::Incompatible`], read from its header by
//! [`crate::arch`] without loading it).
//!
//! CLAP and VST3.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The line the host prints its scan result on; anything else a plug-in prints is ignored.
pub const SCAN_MARKER: &str = "SUHINA-SCAN ";
/// Folder levels a scan goes down.
const MAX_DEPTH: usize = 8;
/// Plug-in files one scan looks at, at most.
const MAX_FILES: usize = 10_000;
/// Bytes of a scanner's output read, at most.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
/// The saved catalog's format. A catalog saved in another one is dropped and everything is read
/// again, except one saved in 2 or 3, which is read (2: plug-ins list what they replace; 3: entries
/// keep when they were last read, and a file that holds no plug-ins is kept as such; 4: a VST3
/// plug-in says whether it supports ARA, so the VST3 files of an older catalog are read again).
const CATALOG_VERSION: u64 = 4;
/// How often a later scan tries again a file that hung: a hang costs its whole timeout each
/// time, so a plug-in that always hangs slows down one scan a day, not every scan.
pub const HANG_RETRY: Duration = Duration::from_secs(24 * 60 * 60);
/// Why a file gave [`Scanned::NoPlugins`], in words.
pub const NO_PLUGINS: &str = "the file holds no plug-ins";
/// How every hang reason starts ([`hung_message`]).
const HUNG: &str = "hung: ";
/// Longest failure reason kept; a loader's message can list every path it tried.
const MAX_REASON: usize = 240;

/// The plug-in formats a folder can be scanned for. Suhina hosts CLAP, VST3 and — behind the
/// host's `vst2` feature (ADR 0022) — VST2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FolderFormat {
    /// Every format Suhina hosts.
    #[default]
    Any,
    Vst3,
    Clap,
    Vst2,
}

impl FolderFormat {
    pub const ALL: [FolderFormat; 4] = [Self::Any, Self::Vst3, Self::Clap, Self::Vst2];

    /// The name it is saved and given as: `any`, `vst3`, `clap` or `vst2`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Vst3 => "vst3",
            Self::Clap => "clap",
            Self::Vst2 => "vst2",
        }
    }

    /// The format [`FolderFormat::name`] gives `text`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == text)
    }

    /// Whether a file named like `path` is a plug-in of this format. A VST2 plug-in is a
    /// `.vst` bundle on macOS, a `.dll` on Windows and a `.so` on Linux.
    fn takes(self, path: &Path) -> bool {
        match self {
            Self::Any => has_extension(path, "clap") || is_vst3(path) || is_vst2(path),
            Self::Vst3 => is_vst3(path),
            Self::Clap => has_extension(path, "clap"),
            Self::Vst2 => is_vst2(path),
        }
    }
}

/// `why` as kept and shown: its first line, at most [`MAX_REASON`] characters.
pub fn short_reason(why: &str) -> String {
    let line = why.lines().next().unwrap_or_default().trim();
    match line.char_indices().nth(MAX_REASON) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
}

/// One plug-in a file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    /// The file (or bundle) that holds it.
    pub path: PathBuf,
    /// The plug-in's own stable id, such as `com.example.synth`.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// What it is, in the format's words (`audio-effect`, `instrument`, `stereo`, …).
    pub features: Vec<String>,
    /// Class ids (32 hex digits) of older plug-ins this one says it replaces and opens the
    /// saved state of: a VST3 plug-in's compatibility list (`moduleinfo.json` or
    /// `IPluginCompatibility`), which may name VST2 plug-ins by their derived class id
    /// ([`crate::matching::vst2_class_id`]).
    pub replaces: Vec<String>,
    /// The plug-in can edit clips over ARA ([ADR 0028](../../../docs/decisions/0028-ara-hosting.md)):
    /// its VST3 module declares an [`ARA_FACTORY_CLASS`] paired with it ([`pair_ara`]).
    pub ara: bool,
}

impl PluginInfo {
    /// Its format: `vst3`, `vst2` or `clap` — from the file's extension, or the name of a
    /// plug-in built into the host for tests.
    pub fn format(&self) -> &'static str {
        let path = self.path.display().to_string();
        if path.starts_with("test:") {
            return match path.as_str() {
                "test:vst3" | "test:vst3-mix" => "vst3",
                "test:vst2" => "vst2",
                _ => "clap",
            };
        }
        if is_vst3(&self.path) {
            "vst3"
        } else if is_vst2(&self.path) {
            "vst2"
        } else {
            "clap"
        }
    }

    /// How the host is asked to load it (`clap:<file>#<id>` or `vst3:<file>#<id>`). A
    /// plug-in built into the host for tests has its name as its file (`test:vst3`) and is
    /// loaded by it.
    pub fn spec(&self) -> String {
        let path = self.path.display().to_string();
        if path.starts_with("test:") {
            return path;
        }
        format!("{}:{}#{}", self.format(), path, self.id)
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

fn is_vst3(path: &Path) -> bool {
    has_extension(path, "vst3")
}

/// A VST2 plug-in: a `.vst` bundle (macOS), a `.dll` (Windows) or a `.so` (Linux) — the
/// shape of this system only.
pub fn is_vst2(path: &Path) -> bool {
    let extension = if cfg!(target_os = "macos") {
        "vst"
    } else if cfg!(windows) {
        "dll"
    } else {
        "so"
    };
    has_extension(path, extension)
}

/// What reading one file gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Scanned {
    Plugins(Vec<PluginInfo>),
    /// The scanner loaded the file and it holds no audio plug-ins (an empty file, or a library
    /// that is not one). Settled: not read again while the file is unchanged, and not listed
    /// among the failures.
    NoPlugins,
    /// The file's code is built only for another processor (its header says so), so it never
    /// loads here: the reason in words ([`crate::arch::reason`]). Settled like
    /// [`Scanned::NoPlugins`], but listed, so the user knows why the plug-in is not offered.
    Incompatible(String),
    /// Why the file gave no plug-ins: it crashed, hung, or could not be loaded. Tried again by
    /// later scans, since the reason may be passing.
    Failed(String),
}

/// One file in the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// Modification time (seconds since 1970) and size when it was read.
    pub modified: u64,
    pub size: u64,
    pub scanned: Scanned,
    /// When it was last read (seconds since 1970; 0 when not known).
    pub tried: u64,
}

impl Entry {
    /// Whether a scan at `now` (seconds since 1970) reads this unchanged file again: a failure
    /// may be passing, so it is, except a hang tried less than [`HANG_RETRY`] ago.
    fn retry_due(&self, now: u64) -> bool {
        match &self.scanned {
            Scanned::Failed(why) => !is_hang(why) || now.saturating_sub(self.tried) >= HANG_RETRY.as_secs(),
            Scanned::Plugins(_) | Scanned::NoPlugins | Scanned::Incompatible(_) => false,
        }
    }
}

/// What a scan did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Files read for the first time or again because they changed.
    pub read: Vec<PathBuf>,
    /// Files whose plug-ins failed to load (read in this scan), after the second try.
    pub failed: Vec<(PathBuf, String)>,
    /// Files that failed in the first pass and were read once more, one at a time.
    pub retried: Vec<PathBuf>,
    /// Files read in this scan that hold no plug-ins ([`Scanned::NoPlugins`]).
    pub ignored: Vec<PathBuf>,
    /// Files read in this scan that are built for another processor, and why
    /// ([`Scanned::Incompatible`]).
    pub incompatible: Vec<(PathBuf, String)>,
    /// Files no longer there, dropped from the catalog.
    pub removed: Vec<PathBuf>,
    /// Files that had not changed since the last scan and were not tried again.
    pub unchanged: usize,
}

/// Every plug-in file found so far, with what reading it gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    entries: Vec<Entry>,
}

/// Where this system keeps CLAP, VST3 and VST2 plug-ins (those that exist), then the folders
/// in `CLAP_PATH`.
pub fn standard_folders() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut folders: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for format in ["CLAP", "VST3"] {
            folders.extend(env("COMMONPROGRAMFILES").map(|p| p.join(format)));
            folders.extend(env("LOCALAPPDATA").map(|p| p.join("Programs").join("Common").join(format)));
        }
        // The VST2 conventions, which no registry holds: the folders installers use.
        folders.extend(env("COMMONPROGRAMFILES").map(|p| p.join("VST2")));
        folders.extend(env("PROGRAMFILES").map(|p| p.join("VSTPlugins")));
        folders.extend(env("COMMONPROGRAMFILES").map(|p| p.join("VSTPlugins")));
        folders.extend(env("PROGRAMFILES").map(|p| p.join("Steinberg").join("VSTPlugins")));
        folders.extend(env("COMMONPROGRAMFILES").map(|p| p.join("Steinberg").join("Shared")));
    } else if cfg!(target_os = "macos") {
        for format in ["CLAP", "VST3"] {
            folders.extend(env("HOME").map(|p| p.join("Library/Audio/Plug-Ins").join(format)));
            folders.push(PathBuf::from("/Library/Audio/Plug-Ins").join(format));
        }
        // A VST2 plug-in is a `.vst` bundle, and its folder is named VST.
        folders.extend(env("HOME").map(|p| p.join("Library/Audio/Plug-Ins/VST")));
        folders.push(PathBuf::from("/Library/Audio/Plug-Ins/VST"));
    } else {
        folders.extend(env("HOME").map(|p| p.join(".clap")));
        folders.push(PathBuf::from("/usr/lib/clap"));
        folders.push(PathBuf::from("/usr/local/lib/clap"));
        folders.extend(env("HOME").map(|p| p.join(".vst3")));
        folders.push(PathBuf::from("/usr/lib/vst3"));
        folders.push(PathBuf::from("/usr/local/lib/vst3"));
        // VST2's: the same libraries, in the vst folders.
        folders.extend(env("HOME").map(|p| p.join(".vst")));
        folders.push(PathBuf::from("/usr/lib/vst"));
        folders.push(PathBuf::from("/usr/local/lib/vst"));
    }
    if let Some(paths) = std::env::var_os("CLAP_PATH") {
        folders.extend(std::env::split_paths(&paths));
    }
    folders.retain(|f| f.is_dir());
    folders.dedup();
    folders
}

/// The plug-in files under `folders`: files or bundle folders named `*.clap` or `*.vst3` (a
/// bundle is not looked into), at most [`MAX_DEPTH`] levels down and [`MAX_FILES`] in all,
/// sorted. A folder that is a symbolic link or junction is looked into like any other (an
/// installer may link a plug-in's folder into the standard one; the depth limit bounds a
/// loop).
pub fn find_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let folders: Vec<(PathBuf, FolderFormat)> = folders.iter().map(|f| (f.clone(), FolderFormat::Any)).collect();
    find_files_of(&folders)
}

/// [`find_files`] with a format for each folder: only that format's files are taken from it.
pub fn find_files_of(folders: &[(PathBuf, FolderFormat)]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending: Vec<(PathBuf, FolderFormat, usize)> =
        folders.iter().map(|(f, format)| (f.clone(), *format, 0)).collect();
    while let Some((folder, format, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if format.takes(&path) {
                found.push(path);
                if found.len() >= MAX_FILES {
                    found.sort();
                    return found;
                }
            } else if depth < MAX_DEPTH && std::fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
                pending.push((path, format, depth + 1));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Modification time and size of `path` (for a bundle folder, of the folder itself).
fn stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    Some((modified, meta.len()))
}

/// Reads `file` in a `host --scan` process, waiting at most `timeout`. `file` may also be a
/// `test:` name the host understands, for tests.
pub fn scan_file(host: &Path, file: &str, timeout: Duration) -> Scanned {
    let child = crate::host_command(host)
        .args(["--scan", file])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Scanned::Failed(format!("could not start the scanner: {e}")),
    };
    let reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.take(MAX_OUTPUT).read_to_string(&mut text);
            text
        })
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = reader.and_then(|r| r.join().ok()).unwrap_or_default();
    let Some(status) = status else {
        return Scanned::Failed(hung_message(timeout));
    };
    let result = output
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(SCAN_MARKER))
        .and_then(|j| serde_json::from_str::<Value>(j).ok());
    match result {
        Some(value) => parse_scan(&value, file),
        None if status.success() => Scanned::Failed("the scanner gave no answer".into()),
        None => Scanned::Failed(format!("crashed while loading ({status})")),
    }
}

/// Why [`scan_file`] gave up on a file: its process did not answer within `timeout` and was
/// ended. Only the scanner's own deadline gives this reason, so a test can tell a hang that was
/// caught from any other failure.
pub fn hung_message(timeout: Duration) -> String {
    format!("{HUNG}no answer within {} s", timeout.as_secs_f32())
}

/// Whether `why` is the reason a hang gives ([`hung_message`]).
pub fn is_hang(why: &str) -> bool {
    why.starts_with(HUNG)
}

/// Seconds since 1970, now.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What the host prints for a scan that found `plugins` (their paths are not printed).
pub fn scan_json(plugins: &[PluginInfo]) -> String {
    let list: Vec<Value> = plugins.iter().map(plugin_json).collect();
    json!({ "plugins": list }).to_string()
}

/// What a scan worker prints for `file`: its plug-ins, or why it has none.
pub fn scan_answer(file: &str, result: Result<&[PluginInfo], &str>) -> String {
    let value = match result {
        Ok(plugins) => json!({ "file": file, "plugins": plugins.iter().map(plugin_json).collect::<Vec<_>>() }),
        Err(message) => json!({ "file": file, "error": message }),
    };
    value.to_string()
}

/// What the host prints for a scan that failed.
pub fn scan_error_json(message: &str) -> String {
    json!({ "error": message }).to_string()
}

fn plugin_json(p: &PluginInfo) -> Value {
    let mut value = json!({
        "id": p.id, "name": p.name, "vendor": p.vendor, "version": p.version, "features": p.features,
        "replaces": p.replaces,
    });
    if p.ara {
        value["ara"] = json!(true);
    }
    value
}

/// The category of a VST3 module's class that makes ARA document controllers (`ARAVST3.h`,
/// `kARAMainFactoryClass`).
pub const ARA_FACTORY_CLASS: &str = "ARA Main Factory Class";

/// Marks the audio classes of one VST3 module that support ARA, given the names of the
/// module's [`ARA_FACTORY_CLASS`] classes. `ARAVST3.h` pairs an ARA factory with the audio
/// processor of the same class name; a module with one of each pairs them whatever their names,
/// as nothing else could be meant.
pub fn pair_ara(plugins: &mut [PluginInfo], ara_classes: &[String]) {
    if ara_classes.is_empty() {
        return;
    }
    if let ([plugin], [_]) = (&mut *plugins, ara_classes) {
        plugin.ara = true;
        return;
    }
    for plugin in plugins.iter_mut() {
        plugin.ara = ara_classes.contains(&plugin.name);
    }
}

/// Fills in [`PluginInfo::replaces`] from a VST3 compatibility list: an array of
/// `{ "New": <class id>, "Old": [<class id>, …] }`, the form both `moduleinfo.json` and
/// `IPluginCompatibility` use. Ids that are not 32 hex digits are skipped.
pub fn apply_compatibility(plugins: &mut [PluginInfo], list: &Value) {
    let class_id = |v: &Value| {
        v.as_str().map(str::to_ascii_uppercase).filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    for entry in list.as_array().into_iter().flatten() {
        let Some(new) = entry.get("New").and_then(class_id) else { continue };
        let old: Vec<String> =
            entry.get("Old").and_then(Value::as_array).into_iter().flatten().filter_map(class_id).collect();
        for plugin in plugins.iter_mut().filter(|p| p.id.eq_ignore_ascii_case(&new)) {
            for id in &old {
                if !plugin.replaces.contains(id) {
                    plugin.replaces.push(id.clone());
                }
            }
        }
    }
}

/// [`apply_compatibility`] from the list's text, as a plug-in writes it. It is read as JSON5,
/// which also takes the trailing commas some lists have; a trailing NUL is ignored and text
/// that cannot be read changes nothing.
pub fn apply_compatibility_text(plugins: &mut [PluginInfo], text: &str) {
    if let Ok(list) = json5::from_str::<Value>(text.trim_end_matches('\0')) {
        apply_compatibility(plugins, &list);
    }
}

/// The strings in the array `value[key]`.
fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|f| f.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

pub(crate) fn parse_scan(value: &Value, file: &str) -> Scanned {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Scanned::Failed(short_reason(error));
    }
    let Some(list) = value.get("plugins").and_then(Value::as_array) else {
        return Scanned::Failed("the scanner's answer has no plug-ins".into());
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let plugins = list
        .iter()
        .filter(|p| !text(p, "id").is_empty())
        .map(|p| PluginInfo {
            path: PathBuf::from(file),
            id: text(p, "id"),
            name: text(p, "name"),
            vendor: text(p, "vendor"),
            version: text(p, "version"),
            features: strings(p, "features"),
            replaces: strings(p, "replaces"),
            ara: p.get("ara").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect::<Vec<_>>();
    if plugins.is_empty() { Scanned::NoPlugins } else { Scanned::Plugins(plugins) }
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Adds the plug-ins `path` holds as if a scan had read them (plug-ins described some other
    /// way, and tests with the host's own test plug-ins). Replaces what was known about `path`.
    pub fn insert(&mut self, path: PathBuf, plugins: Vec<PluginInfo>) {
        self.entries.retain(|e| e.path != path);
        self.entries.push(Entry { path, modified: 0, size: 0, scanned: Scanned::Plugins(plugins), tried: 0 });
    }

    /// Marks the plug-in `spec` ([`PluginInfo::spec`]) as one that edits clips over ARA, as its
    /// host found when it loaded (a scan from before Suhina read ARA, or one that missed it).
    /// Returns whether anything changed.
    pub fn mark_ara(&mut self, spec: &str) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if let Scanned::Plugins(plugins) = &mut entry.scanned {
                for plugin in plugins.iter_mut().filter(|p| !p.ara && p.spec() == spec) {
                    plugin.ara = true;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Every plug-in found, in file order.
    pub fn plugins(&self) -> impl Iterator<Item = &PluginInfo> {
        self.entries
            .iter()
            .filter_map(|e| match &e.scanned {
                Scanned::Plugins(p) => Some(p.iter()),
                Scanned::NoPlugins | Scanned::Incompatible(_) | Scanned::Failed(_) => None,
            })
            .flatten()
    }

    /// The files whose plug-ins failed to load, with the reason.
    pub fn failed(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.entries.iter().filter_map(|e| match &e.scanned {
            Scanned::Failed(why) => Some((e.path.as_path(), why.as_str())),
            Scanned::Plugins(_) | Scanned::NoPlugins | Scanned::Incompatible(_) => None,
        })
    }

    /// The files built only for another processor, with the reason.
    pub fn incompatible(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.entries.iter().filter_map(|e| match &e.scanned {
            Scanned::Incompatible(why) => Some((e.path.as_path(), why.as_str())),
            Scanned::Plugins(_) | Scanned::NoPlugins | Scanned::Failed(_) => None,
        })
    }

    /// The files that hold no plug-ins ([`Scanned::NoPlugins`]).
    pub fn ignored(&self) -> impl Iterator<Item = &Path> {
        self.entries.iter().filter(|e| e.scanned == Scanned::NoPlugins).map(|e| e.path.as_path())
    }

    /// The catalog without the plug-ins `off` picks (the ones the user switched off), as
    /// sessions see it: they are not offered, and a project that uses one finds it missing.
    pub fn without(&self, off: impl Fn(&PluginInfo) -> bool) -> Catalog {
        let mut catalog = self.clone();
        for entry in &mut catalog.entries {
            if let Scanned::Plugins(plugins) = &mut entry.scanned {
                plugins.retain(|p| !off(p));
            }
        }
        catalog
    }

    /// The plug-in with this id, or else the first whose name matches (ignoring case).
    pub fn find(&self, id_or_name: &str) -> Option<&PluginInfo> {
        self.plugins()
            .find(|p| p.id == id_or_name)
            .or_else(|| self.plugins().find(|p| p.name.eq_ignore_ascii_case(id_or_name)))
    }

    /// Looks through `folders`, reads new and changed files, and drops files that are gone.
    /// VST3 bundles with a `moduleinfo.json` are read from it; the rest are loaded by
    /// [`scan_pool::default_workers`](crate::scan_pool::default_workers) worker processes, each
    /// file within `timeout` (shells within [`SHELL_TIMEOUT`](crate::scan_pool::SHELL_TIMEOUT)),
    /// and the files that failed are read once more, one at a time
    /// ([`scan_files_retrying`](crate::scan_pool::scan_files_retrying)).
    ///
    /// Unchanged files that failed in an earlier scan are read again too, since the reason may
    /// have passed; one that hung, at most once per [`HANG_RETRY`]. An unchanged file that holds
    /// no plug-ins is not, nor one built for another processor; a file built for another
    /// processor is not loaded at all.
    pub fn scan(&mut self, host: &Path, folders: &[PathBuf], timeout: Duration) -> Report {
        let folders: Vec<(PathBuf, FolderFormat)> = folders.iter().map(|f| (f.clone(), FolderFormat::Any)).collect();
        self.scan_folders(host, &folders, timeout)
    }

    /// [`Catalog::scan`] with a format for each folder ([`find_files_of`]).
    pub fn scan_folders(&mut self, host: &Path, folders: &[(PathBuf, FolderFormat)], timeout: Duration) -> Report {
        let now = now();
        let files = find_files_of(folders);
        let mut report = Report::default();
        self.entries.retain(|e| {
            let keep = files.contains(&e.path);
            if !keep {
                report.removed.push(e.path.clone());
            }
            keep
        });
        let mut to_read: Vec<(PathBuf, u64, u64)> = Vec::new();
        for file in files {
            let Some((modified, size)) = stamp(&file) else { continue };
            let same = self.entries.iter().find(|e| e.path == file && e.modified == modified && e.size == size);
            if same.is_some_and(|e| !e.retry_due(now)) {
                report.unchanged += 1;
                continue;
            }
            to_read.push((file, modified, size));
        }
        let fresh = read_files(host, to_read, timeout, now, &mut report);
        self.take_in(fresh, &mut report);
        report
    }

    /// Reads `file` again now, whatever the catalog knew of it (the manager's **Scan again** on
    /// one file). `Err` when it is not a plug-in file the catalog lists or it is gone.
    pub fn scan_file_again(&mut self, host: &Path, file: &Path, timeout: Duration) -> Result<Report, String> {
        if !self.entries.iter().any(|e| e.path == file) {
            return Err(format!("{} is not in the plug-in catalog", file.display()));
        }
        let (modified, size) = stamp(file).ok_or_else(|| format!("{} is gone", file.display()))?;
        let mut report = Report::default();
        let fresh = read_files(host, vec![(file.to_path_buf(), modified, size)], timeout, now(), &mut report);
        self.take_in(fresh, &mut report);
        Ok(report)
    }

    /// Puts what was read into the catalog and the report.
    fn take_in(&mut self, fresh: Vec<Entry>, report: &mut Report) {
        for entry in fresh {
            match &entry.scanned {
                Scanned::Failed(why) => report.failed.push((entry.path.clone(), why.clone())),
                Scanned::Incompatible(why) => report.incompatible.push((entry.path.clone(), why.clone())),
                Scanned::NoPlugins => report.ignored.push(entry.path.clone()),
                Scanned::Plugins(_) => {}
            }
            report.read.push(entry.path.clone());
            match self.entries.iter_mut().find(|e| e.path == entry.path) {
                Some(slot) => *slot = entry,
                None => self.entries.push(entry),
            }
        }
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        report.read.sort();
    }

    /// The catalog as JSON, for saving.
    pub fn to_json(&self) -> String {
        let entries: Vec<Value> = self
            .entries
            .iter()
            .map(|e| {
                let mut value = json!({
                    "path": e.path.display().to_string(), "modified": e.modified, "size": e.size, "tried": e.tried,
                });
                match &e.scanned {
                    Scanned::Plugins(plugins) => {
                        value["plugins"] = plugins.iter().map(plugin_json).collect();
                    }
                    Scanned::NoPlugins => value["plugins"] = json!([]),
                    Scanned::Incompatible(why) => value["incompatible"] = json!(why),
                    Scanned::Failed(why) => value["failed"] = json!(why),
                }
                value
            })
            .collect();
        json!({ "version": CATALOG_VERSION, "entries": entries }).to_string()
    }

    /// A catalog saved by [`Catalog::to_json`]; anything unreadable gives an empty catalog, so
    /// the next scan reads everything again. One saved in version 2 is read: when its entries
    /// were last read is not known, and its "holds no plug-ins" failures are
    /// [`Scanned::NoPlugins`]. In one saved before version 4 the VST3 files that hold plug-ins
    /// count as changed, so the next scan reads them again and learns which support ARA.
    pub fn from_json(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(text) else { return Self::default() };
        let version = value.get("version").and_then(Value::as_u64);
        if !matches!(version, Some(2..=CATALOG_VERSION)) {
            return Self::default();
        }
        let before_ara = version < Some(4);
        let entries = value
            .get("entries")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|e| {
                        let path = PathBuf::from(e.get("path")?.as_str()?);
                        let mut modified = e.get("modified")?.as_u64()?;
                        let size = e.get("size")?.as_u64()?;
                        let tried = e.get("tried").and_then(Value::as_u64).unwrap_or(0);
                        let incompatible = e.get("incompatible").and_then(Value::as_str);
                        let scanned = match e.get("failed").and_then(Value::as_str) {
                            _ if incompatible.is_some() => Scanned::Incompatible(short_reason(incompatible?)),
                            Some(NO_PLUGINS) if version == Some(2) => Scanned::NoPlugins,
                            Some(why) => Scanned::Failed(short_reason(why)),
                            None => match parse_scan(e, &path.display().to_string()) {
                                Scanned::Failed(_) => return None,
                                scanned => scanned,
                            },
                        };
                        if before_ara && is_vst3(&path) && matches!(scanned, Scanned::Plugins(_)) {
                            modified = 0;
                        }
                        Some(Entry { path, modified, size, scanned, tried })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { entries }
    }
}

/// Reads `files` (path, modification time, size): a file built for another processor is settled
/// from its header, a VST3 bundle with a `moduleinfo.json` is read from it, and the rest are
/// loaded by worker processes, the failures once more one at a time.
fn read_files(
    host: &Path,
    files: Vec<(PathBuf, u64, u64)>,
    timeout: Duration,
    now: u64,
    report: &mut Report,
) -> Vec<Entry> {
    use crate::scan_pool::{default_workers, module_info, scan_files_retrying, timeout_for};
    let mut fresh: Vec<Entry> = Vec::new();
    let mut to_load: Vec<(PathBuf, u64, u64)> = Vec::new();
    for (file, modified, size) in files {
        let scanned = match crate::arch::incompatible(&file) {
            Some(archs) => Scanned::Incompatible(crate::arch::reason(&archs)),
            None => match module_info(&file) {
                Some(plugins) => Scanned::Plugins(plugins),
                None => {
                    to_load.push((file, modified, size));
                    continue;
                }
            },
        };
        fresh.push(Entry { path: file, modified, size, scanned, tried: now });
    }
    let names: Vec<String> = to_load.iter().map(|(f, _, _)| f.display().to_string()).collect();
    let (loaded, retried) = scan_files_retrying(host, &names, default_workers(), &|f| timeout_for(f, timeout));
    report.retried = retried.iter().filter_map(|&i| to_load.get(i)).map(|(f, _, _)| f.clone()).collect();
    for ((file, modified, size), mut scanned) in to_load.into_iter().zip(loaded) {
        if let Scanned::Plugins(plugins) = &mut scanned {
            plugins.iter_mut().for_each(|p| p.path = file.clone());
        }
        fresh.push(Entry { path: file, modified, size, scanned, tried: now });
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str) -> PluginInfo {
        PluginInfo {
            path: PathBuf::from("/vst3/Thing.vst3"),
            id: id.into(),
            name: "Thing".into(),
            vendor: "Maker".into(),
            version: "1".into(),
            features: Vec::new(),
            replaces: Vec::new(),
            ara: false,
        }
    }

    #[test]
    fn a_compatibility_list_is_read_as_plugins_write_it() {
        let new = "ABCDEF019182FAEB49746D78426E6477";
        let mut plugins = [plugin(new), plugin("00000000000000000000000000000001")];
        // Lower-case ids, trailing commas and a trailing NUL, as some plug-ins write it; a
        // malformed id and a duplicate are left out.
        let text = format!(
            "[ {{ \"New\": \"{}\", \"Old\": [ \"5653544761696e6761696e0000000000\", \"short\", \
             \"5653544761696E6761696E0000000000\", ], }}, ]\0",
            new.to_lowercase()
        );
        apply_compatibility_text(&mut plugins, &text);
        assert_eq!(plugins[0].replaces, ["5653544761696E6761696E0000000000"]);
        assert!(plugins[1].replaces.is_empty());
        // Text that is not a list changes nothing.
        apply_compatibility_text(&mut plugins, "{ broken");
        assert_eq!(plugins[0].replaces.len(), 1);
    }

    fn named(name: &str) -> PluginInfo {
        PluginInfo { name: name.into(), ..plugin("ABCDEF019182FAEB49746D78426E6477") }
    }

    #[test]
    fn an_ara_factory_pairs_with_the_audio_class_of_its_name() {
        // Two audio classes, one ARA factory named after the second (ARAVST3.h's pairing).
        let mut plugins = [named("Tune"), named("Tune ARA")];
        pair_ara(&mut plugins, &["Tune ARA".into()]);
        assert_eq!(plugins.map(|p| p.ara), [false, true]);
        // One of each pair whatever the names; none makes nothing ARA.
        let mut one = [named("Tune")];
        pair_ara(&mut one, &["Tune (ARA)".into()]);
        assert!(one[0].ara);
        let mut none = [named("Tune")];
        pair_ara(&mut none, &[]);
        assert!(!none[0].ara);
        // Two factories, neither named after the class: not guessed.
        let mut unpaired = [named("Tune"), named("Pitch")];
        pair_ara(&mut unpaired, &["A".into(), "B".into()]);
        assert_eq!(unpaired.map(|p| p.ara), [false, false]);
    }

    #[test]
    fn the_catalog_keeps_ara_and_reads_older_vst3_entries_again() {
        let mut catalog = Catalog::new();
        let ara = PluginInfo { ara: true, ..plugin("ABCDEF019182FAEB49746D78426E6477") };
        catalog.insert(ara.path.clone(), vec![ara.clone()]);
        let text = catalog.to_json();
        assert!(text.contains("\"ara\":true"));
        let read = Catalog::from_json(&text);
        assert_eq!(read.plugins().next(), Some(&ara));
        assert!(read.entries().iter().all(|e| e.modified == catalog.entries()[0].modified));

        // A version 3 catalog: no flag, so ARA is false, and its VST3 file counts as changed so
        // the next scan reads it again; a CLAP file and a failure are kept as they were.
        let old = r#"{ "version": 3, "entries": [
            { "path": "/vst3/Thing.vst3", "modified": 7, "size": 9, "tried": 5,
              "plugins": [ { "id": "ABCDEF019182FAEB49746D78426E6477", "name": "Thing" } ] },
            { "path": "/clap/Thing.clap", "modified": 7, "size": 9, "tried": 5,
              "plugins": [ { "id": "com.example.thing", "name": "Thing" } ] },
            { "path": "/vst3/Broken.vst3", "modified": 7, "size": 9, "tried": 5, "failed": "it crashed" } ] }"#;
        let read = Catalog::from_json(old);
        let modified: Vec<u64> = read.entries().iter().map(|e| e.modified).collect();
        let paths: Vec<String> = read.entries().iter().map(|e| e.path.display().to_string()).collect();
        let at = |name: &str| paths.iter().position(|p| p.ends_with(name)).map(|i| modified[i]);
        assert_eq!((at("Thing.vst3"), at("Thing.clap"), at("Broken.vst3")), (Some(0), Some(7), Some(7)));
        assert!(read.plugins().all(|p| !p.ara));
        // An unknown version is still dropped.
        assert!(Catalog::from_json(r#"{ "version": 9, "entries": [] }"#).entries().is_empty());
    }
}
