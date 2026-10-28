// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The user's plug-in library: the folders they added (each for one format, or for every
//! format Suhina hosts) and the folders switched off, the catalog of what the last scan found,
//! the plug-ins switched off, and how each plug-in is hosted. All are kept as JSON in Suhina's
//! settings folder, so a scan survives a restart and the next one reads only what changed.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::scan::{Catalog, FolderFormat, PluginInfo, Report, standard_folders};

/// How long loading one plug-in file may take before it counts as hung (shells get longer).
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
/// Folders a user may add.
const MAX_FOLDERS: usize = 256;
const FOLDERS_FILE: &str = "plugin-folders.json";
const CATALOG_FILE: &str = "plugin-catalog.json";
const HOSTING_FILE: &str = "plugin-hosting.json";
const DISABLED_FILE: &str = "plugin-disabled.json";
/// Plug-ins whose hosting may be set apart from the default.
const MAX_HOSTING: usize = 4096;
/// Longest hosting file read; a longer one is not Suhina's and is ignored.
const MAX_HOSTING_BYTES: u64 = 1 << 20;
/// Longest plug-in name (its spec) a hosting choice is kept for.
const MAX_SPEC: usize = 4096;

/// A folder scans look in, as the Settings page and `plugin.folders` list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub path: PathBuf,
    /// The format of the plug-ins taken from it.
    pub format: FolderFormat,
    /// Scans look in it. A folder switched off stays listed; its plug-ins leave the catalog with
    /// the next scan.
    pub enabled: bool,
    /// One of the system's standard folders: it can be switched off, not removed.
    pub standard: bool,
}

/// The format a standard folder holds, from its name (`…/VST3`, `…/CLAP`, `~/.clap`).
fn standard_format(path: &Path) -> FolderFormat {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match name.trim_start_matches('.') {
        "vst3" => FolderFormat::Vst3,
        "clap" => FolderFormat::Clap,
        _ => FolderFormat::Any,
    }
}

/// Which host process a plug-in's instances run in (ADR 0006, decision 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hosting {
    /// One process per plug-in file: every instance of every plug-in in the file shares it.
    #[default]
    Binary,
    /// A process of its own for every instance.
    Instance,
    /// One process per vendor: every instance of the vendor's plug-ins shares it.
    Vendor,
}

impl Hosting {
    /// The name it is saved and shown as: `binary`, `instance` or `vendor`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Instance => "instance",
            Self::Vendor => "vendor",
        }
    }

    /// The hosting [`Hosting::name`] gives `text`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "binary" => Some(Self::Binary),
            "instance" => Some(Self::Instance),
            "vendor" => Some(Self::Vendor),
            _ => None,
        }
    }
}

/// The process group ([`crate::Launch::group`]) an instance of `plugin` starts in under
/// `hosting`: its file's (`file:<path>`, shared by every plug-in in the file or shell), its
/// vendor's (`vendor:<name>`, or the file's when the vendor is unknown), or none (a process of
/// its own).
pub fn group_for(hosting: Hosting, plugin: &PluginInfo) -> Option<String> {
    let file = || format!("file:{}", plugin.path.display());
    match hosting {
        Hosting::Binary => Some(file()),
        Hosting::Vendor if plugin.vendor.trim().is_empty() => Some(file()),
        Hosting::Vendor => Some(format!("vendor:{}", plugin.vendor.trim())),
        // An ARA plug-in's instances and the document they edit live in one process
        // (ADR 0028 decision 2), so a choice saved before the plug-in was known to support ARA
        // gives way to the default.
        Hosting::Instance if plugin.ara => Some(file()),
        Hosting::Instance => None,
    }
}

/// Suhina's settings folder: `%APPDATA%\Suhina` on Windows,
/// `~/Library/Application Support/Suhina` on macOS, `$XDG_CONFIG_HOME/suhina` (or
/// `~/.config/suhina`) elsewhere. `None` when the system says nothing about where.
/// While only the folder from before the rename exists (copying it under the new name failed,
/// say because security software stopped the write), that one, so the settings stay readable.
pub fn settings_folder() -> Option<PathBuf> {
    prefer_new(new_settings_folder(), old_settings_folder())
}

/// The settings folder under the program's current name, whether or not it exists yet.
pub fn new_settings_folder() -> Option<PathBuf> {
    settings_folder_named("Suhina")
}

/// The settings folder from before the program was renamed Suhina, under its old name.
pub fn old_settings_folder() -> Option<PathBuf> {
    settings_folder_named("Loopwerk") // rename_suhina: keep
}

fn settings_folder_named(name: &str) -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env("APPDATA").map(|p| p.join(name))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library/Application Support").join(name))
    } else {
        let config = env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|p| p.join(".config")));
        config.map(|p| p.join(name.to_lowercase()))
    }
}

/// `new`, or `old` while only `old` exists.
pub fn prefer_new(new: Option<PathBuf>, old: Option<PathBuf>) -> Option<PathBuf> {
    match (&new, &old) {
        (Some(n), Some(o)) if !n.exists() && o.is_dir() => old,
        _ => new,
    }
}

/// Copies the folder `old` to `new` when `old` exists and `new` does not, so the settings kept
/// under the program's old name carry over while builds from before the rename keep theirs.
/// The copy is made beside `new` and renamed into place, so a failed one is tried again on the
/// next start. A warning when the copy fails; nothing to do is not one.
pub fn adopt_old_folder(old: Option<PathBuf>, new: Option<PathBuf>) -> Option<String> {
    let (old, new) = (old?, new?);
    if !old.is_dir() || new.exists() {
        return None;
    }
    let partial = new.with_extension("partial");
    let _ = std::fs::remove_dir_all(&partial);
    let copied = new
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| copy_folder(&old, &partial))
        .and_then(|()| std::fs::rename(&partial, &new));
    copied.err().map(|e| {
        let _ = std::fs::remove_dir_all(&partial);
        format!("could not copy the old settings {} to {}: {e}", old.display(), new.display())
    })
}

/// Copies the files and folders in `from` to `to`; links are left out.
fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The `suhina-plugin-host` executable next to the running one, past a link to it (on macOS the
/// package's `suhina-cli` links to the one inside `Suhina.app`).
pub fn host_next_to_this_program() -> PathBuf {
    let name = format!("suhina-plugin-host{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .map(|exe| exe.canonicalize().unwrap_or(exe))
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// `path` without Windows' `\\?\` prefix where it is an ordinary drive path, as people write it.
pub fn plain(path: PathBuf) -> PathBuf {
    let rest = {
        let text = path.to_string_lossy();
        text.strip_prefix(r"\\?\").filter(|rest| rest.as_bytes().get(1) == Some(&b':')).map(PathBuf::from)
    };
    rest.unwrap_or(path)
}

/// A leading `~` alone or before a separator becomes `home`, so a folder typed with one is the
/// folder it names. `~name` (another user's home) stays as typed; without a home folder, too.
fn expand_home(folder: &Path, home: Option<&OsStr>) -> PathBuf {
    let Some(home) = home else { return folder.to_path_buf() };
    match folder.strip_prefix("~") {
        Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from(home),
        // `~name` is another user's home: not ours to expand.
        Ok(rest) if !rest.is_absolute() => Path::new(home).join(rest),
        _ => folder.to_path_buf(),
    }
}

/// The home folder a typed `~` becomes: `HOME`, and on Windows also `USERPROFILE`, which Windows
/// sets where `HOME` is often absent (only shells set `HOME` there).
fn user_home() -> Option<OsString> {
    home_folder(std::env::var_os("HOME"), std::env::var_os("USERPROFILE"))
}

/// `home`, or where it is missing on Windows, `profile`; on other systems only `HOME` counts, as
/// before.
fn home_folder(home: Option<OsString>, profile: Option<OsString>) -> Option<OsString> {
    if cfg!(windows) { home.or(profile) } else { home }
}

/// The folders the user added and the catalog, loaded from the settings folder on first use.
#[derive(Debug, Clone)]
pub struct Library {
    folder: Option<PathBuf>,
    host: PathBuf,
    loaded: bool,
    /// The folders the user added, with the format of each.
    custom: Vec<(PathBuf, FolderFormat)>,
    /// Folders (standard or added) scans do not look in.
    off: Vec<PathBuf>,
    catalog: Catalog,
    /// Plug-ins (by spec) the user switched off: not offered, not loaded.
    disabled: BTreeSet<String>,
    /// Whether scans look in the system's standard folders as well as the added ones.
    standard: bool,
    /// Plug-ins (by spec) hosted other than the default way.
    hosting: BTreeMap<String, Hosting>,
}

impl Default for Library {
    /// The library in Suhina's settings folder, scanned with the host next to this program.
    fn default() -> Self {
        Self::new(settings_folder(), host_next_to_this_program())
    }
}

impl Library {
    /// A library kept in `folder` (nothing is saved when it is `None`), scanned with `host`.
    pub fn new(folder: Option<PathBuf>, host: PathBuf) -> Self {
        Self {
            folder,
            host,
            loaded: false,
            custom: Vec::new(),
            off: Vec::new(),
            catalog: Catalog::new(),
            disabled: BTreeSet::new(),
            standard: true,
            hosting: BTreeMap::new(),
        }
    }

    /// Scans only the folders the user added, not the system's standard ones (tests, portable
    /// setups).
    pub fn without_standard_folders(mut self) -> Self {
        self.standard = false;
        self
    }

    pub fn host(&self) -> &Path {
        &self.host
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(folder) = &self.folder else { return };
        if let Ok(text) = std::fs::read_to_string(folder.join(FOLDERS_FILE)) {
            (self.custom, self.off) = parse_folders(&text);
        }
        if let Ok(text) = std::fs::read_to_string(folder.join(CATALOG_FILE)) {
            self.catalog = Catalog::from_json(&text);
        }
        if let Some(text) = read_bounded(&folder.join(HOSTING_FILE), MAX_HOSTING_BYTES) {
            self.hosting = parse_hosting(&text);
        }
        if let Some(text) = read_bounded(&folder.join(DISABLED_FILE), MAX_HOSTING_BYTES) {
            self.disabled = parse_disabled(&text);
        }
    }

    fn save(&self, name: &str, text: &str) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("this system has no settings folder to save in")?;
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = folder.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The folders the user added, in the order added.
    pub fn custom_folders(&mut self) -> Vec<PathBuf> {
        self.load();
        self.custom.iter().map(|(path, _)| path.clone()).collect()
    }

    /// Every folder scans may look in: the system's standard ones, then the user's, each with
    /// its format and whether it is switched on.
    pub fn folder_list(&mut self) -> Vec<Folder> {
        self.load();
        let mut list: Vec<Folder> = Vec::new();
        let standard = if self.standard { standard_folders() } else { Vec::new() };
        for path in standard {
            let format = standard_format(&path);
            list.push(Folder { enabled: !self.off.contains(&path), path, format, standard: true });
        }
        for (path, format) in &self.custom {
            if !list.iter().any(|f| f.path == *path) {
                let enabled = !self.off.contains(path);
                list.push(Folder { path: path.clone(), format: *format, enabled, standard: false });
            }
        }
        list
    }

    /// The folders a scan looks in now (switched on), with their formats.
    pub fn folders(&mut self) -> Vec<(PathBuf, FolderFormat)> {
        self.folder_list().into_iter().filter(|f| f.enabled).map(|f| (f.path, f.format)).collect()
    }

    /// Adds a folder to look in for every format Suhina hosts; returns false if it was there
    /// already.
    pub fn add_folder(&mut self, folder: &Path) -> Result<bool, String> {
        self.add_folder_as(folder, FolderFormat::Any)
    }

    /// Adds a folder to look in for `format`'s plug-ins; returns false if it was there already
    /// with that format (with another one, the format changes). A folder typed with a leading
    /// `~` works as typed: the home folder takes its place.
    pub fn add_folder_as(&mut self, folder: &Path, format: FolderFormat) -> Result<bool, String> {
        self.load();
        let folder = expand_home(folder, user_home().as_deref());
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        let folder = folder.canonicalize().map(plain).unwrap_or_else(|_| folder.to_path_buf());
        if let Some(slot) = self.custom.iter_mut().find(|(path, _)| *path == folder) {
            if slot.1 == format {
                return Ok(false);
            }
            slot.1 = format;
            self.save_folders()?;
            return Ok(true);
        }
        if self.custom.len() >= MAX_FOLDERS {
            return Err(format!("at most {MAX_FOLDERS} folders can be added"));
        }
        self.custom.push((folder, format));
        self.save_folders()?;
        Ok(true)
    }

    /// Stops looking in a folder the user added; returns false if it was not one of them.
    pub fn remove_folder(&mut self, folder: &Path) -> Result<bool, String> {
        self.load();
        let canonical = folder.canonicalize().ok().map(plain);
        let matches = |f: &PathBuf| f == folder || Some(f) == canonical.as_ref();
        let before = self.custom.len();
        self.custom.retain(|(f, _)| !matches(f));
        if self.custom.len() == before {
            return Ok(false);
        }
        self.off.retain(|f| !matches(f));
        self.save_folders()?;
        Ok(true)
    }

    /// Switches a folder (a standard one or an added one) on or off for scans; returns false
    /// when it already was so.
    pub fn set_folder_enabled(&mut self, folder: &Path, enabled: bool) -> Result<bool, String> {
        let listed = self.folder_list();
        let canonical = folder.canonicalize().ok().map(plain);
        let path = listed
            .iter()
            .find(|f| f.path == folder || Some(&f.path) == canonical.as_ref())
            .map(|f| f.path.clone())
            .ok_or_else(|| format!("{} is not one of the plug-in folders", folder.display()))?;
        let was = !self.off.contains(&path);
        if was == enabled {
            return Ok(false);
        }
        if enabled {
            self.off.retain(|f| *f != path);
        } else {
            self.off.push(path);
        }
        self.save_folders()?;
        Ok(true)
    }

    fn save_folders(&self) -> Result<(), String> {
        let added: Vec<Value> = self
            .custom
            .iter()
            .map(|(path, format)| json!({ "path": path.display().to_string(), "format": format.name() }))
            .collect();
        let off: Vec<String> = self.off.iter().map(|f| f.display().to_string()).collect();
        self.save(FOLDERS_FILE, &json!({ "added": added, "off": off }).to_string())
    }

    /// Whether the plug-in `plugin_spec` ([`PluginInfo::spec`]) is switched on (the default).
    pub fn is_enabled(&mut self, plugin_spec: &str) -> bool {
        self.load();
        !self.disabled.contains(plugin_spec)
    }

    /// Switches the plug-in `plugin_spec` on or off, and saves it. One switched off is not
    /// offered in the browser or found by name, and a project that uses it finds it missing.
    pub fn set_enabled(&mut self, plugin_spec: &str, enabled: bool) -> Result<(), String> {
        self.load();
        if plugin_spec.is_empty() || plugin_spec.len() > MAX_SPEC {
            return Err("not a plug-in name".into());
        }
        if enabled {
            self.disabled.remove(plugin_spec);
        } else {
            if !self.disabled.contains(plugin_spec) && self.disabled.len() >= MAX_HOSTING {
                return Err(format!("at most {MAX_HOSTING} plug-ins can be switched off"));
            }
            self.disabled.insert(plugin_spec.to_string());
        }
        let list: Vec<&String> = self.disabled.iter().collect();
        self.save(DISABLED_FILE, &json!(list).to_string())
    }

    /// The catalog without the plug-ins switched off: what sessions offer and load.
    pub fn enabled_catalog(&mut self) -> Catalog {
        self.load();
        let disabled = &self.disabled;
        self.catalog.without(|p| disabled.contains(&p.spec()))
    }

    /// Reads one file of the catalog again now, and saves the catalog.
    pub fn scan_file_again(&mut self, file: &Path) -> Result<Report, String> {
        self.load();
        if !self.host.exists() {
            return Err(format!("the plug-in host {} is missing; it ships next to Suhina", self.host.display()));
        }
        let report = self.catalog.scan_file_again(&self.host, file, SCAN_TIMEOUT)?;
        self.save(CATALOG_FILE, &self.catalog.to_json())?;
        Ok(report)
    }

    /// Scans every folder and saves the catalog. With `all`, forgets what earlier scans learned
    /// first, so every file is read again (failed ones too).
    pub fn scan(&mut self, all: bool) -> Result<Report, String> {
        self.load();
        if !self.host.exists() {
            return Err(format!("the plug-in host {} is missing; it ships next to Suhina", self.host.display()));
        }
        if all {
            self.catalog = Catalog::new();
        }
        let folders = self.folders();
        let report = self.catalog.scan_folders(&self.host, &folders, SCAN_TIMEOUT);
        self.save(CATALOG_FILE, &self.catalog.to_json())?;
        Ok(report)
    }

    /// Remembers that the plug-in `spec` edits clips over ARA ([`Catalog::mark_ara`]) and saves
    /// the catalog. Returns whether the catalog changed.
    pub fn mark_ara(&mut self, spec: &str) -> Result<bool, String> {
        self.load();
        if !self.catalog.mark_ara(spec) {
            return Ok(false);
        }
        self.save(CATALOG_FILE, &self.catalog.to_json())?;
        Ok(true)
    }

    /// What the last scan found.
    pub fn catalog(&mut self) -> &Catalog {
        self.load();
        &self.catalog
    }

    /// How the plug-in `plugin_spec` ([`PluginInfo::spec`]) is hosted; [`Hosting::Binary`]
    /// unless the user chose otherwise.
    pub fn hosting(&mut self, plugin_spec: &str) -> Hosting {
        self.load();
        self.hosting.get(plugin_spec).copied().unwrap_or_default()
    }

    /// Chooses how the plug-in `plugin_spec` is hosted from its next start, and saves it.
    pub fn set_hosting(&mut self, plugin_spec: &str, hosting: Hosting) -> Result<(), String> {
        self.load();
        if plugin_spec.is_empty() || plugin_spec.len() > MAX_SPEC {
            return Err("not a plug-in name".into());
        }
        if hosting == Hosting::default() {
            self.hosting.remove(plugin_spec);
        } else {
            if !self.hosting.contains_key(plugin_spec) && self.hosting.len() >= MAX_HOSTING {
                return Err(format!("at most {MAX_HOSTING} plug-ins can be hosted other than the default way"));
            }
            self.hosting.insert(plugin_spec.to_string(), hosting);
        }
        let list: Map<String, Value> =
            self.hosting.iter().map(|(spec, hosting)| (spec.clone(), Value::from(hosting.name()))).collect();
        self.save(HOSTING_FILE, &Value::Object(list).to_string())
    }
}

/// The text of the file at `path`, when it is there and at most `limit` bytes long.
fn read_bounded(path: &Path, limit: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= limit).then_some(text)
}

/// The added folders and the folders switched off in a `plugin-folders.json` text: an object
/// `{ "added": [{ "path", "format" }], "off": [path] }`, or the earlier form, a list of paths
/// (each for every format). Entries that are not that are skipped.
fn parse_folders(text: &str) -> (Vec<(PathBuf, FolderFormat)>, Vec<PathBuf>) {
    let paths = |list: Option<&Value>| -> Vec<PathBuf> {
        list.and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Value::as_str).map(PathBuf::from).take(MAX_FOLDERS).collect())
            .unwrap_or_default()
    };
    match serde_json::from_str::<Value>(text) {
        Ok(list @ Value::Array(_)) => {
            (paths(Some(&list)).into_iter().map(|p| (p, FolderFormat::Any)).collect(), Vec::new())
        }
        Ok(Value::Object(folders)) => {
            let added = folders
                .get("added")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|f| {
                            let path = PathBuf::from(f.get("path")?.as_str()?);
                            let format = f.get("format").and_then(Value::as_str).and_then(FolderFormat::parse);
                            Some((path, format.unwrap_or_default()))
                        })
                        .take(MAX_FOLDERS)
                        .collect()
                })
                .unwrap_or_default();
            (added, paths(folders.get("off")))
        }
        _ => (Vec::new(), Vec::new()),
    }
}

/// The plug-ins switched off in a `plugin-disabled.json` text: a list of specs.
fn parse_disabled(text: &str) -> BTreeSet<String> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else { return BTreeSet::new() };
    list.iter()
        .filter_map(Value::as_str)
        .filter(|spec| !spec.is_empty() && spec.len() <= MAX_SPEC)
        .take(MAX_HOSTING)
        .map(str::to_string)
        .collect()
}

/// The hosting choices in a `plugin-hosting.json` text: an object from plug-in spec to hosting
/// name. Entries that are not that are skipped, and so are the default ones.
fn parse_hosting(text: &str) -> BTreeMap<String, Hosting> {
    let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(text) else {
        return BTreeMap::new();
    };
    entries
        .iter()
        .filter(|(spec, _)| !spec.is_empty() && spec.len() <= MAX_SPEC)
        .filter_map(|(spec, hosting)| Some((spec.clone(), Hosting::parse(hosting.as_str()?)?)))
        .filter(|(_, hosting)| *hosting != Hosting::default())
        .take(MAX_HOSTING)
        .collect()
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
