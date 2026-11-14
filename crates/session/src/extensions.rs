// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Extensions ([ADR 0015](../decisions/0015-extensions.md)): optional components Suhina does not
//! ship, kept as folders under the settings' `extensions/` (`SUHINA_EXTENSIONS` names another
//! root), each with an `extension.json` manifest read as hostile input and validated per
//! [`docs/spec/extensions.md`](../../docs/spec/extensions.md). The `extensions.*` commands are
//! the one interface, the same from the window, the CLI and MCP (ADR 0002). What runs a surface
//! program's script is in [`programs`]: the shipped Lua runner, one process per program
//! ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md) › 10 and its amendment).
//!
//! Behaviour spec: [`docs/spec/extensions.md`](../../docs/spec/extensions.md) and
//! [`docs/spec/surface-programs.md`](../../docs/spec/surface-programs.md).

pub mod programs;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::CommandError;
use crate::commands::{Args, CommandKind, CommandSpec, ParamType, param};
use crate::{Agents, Policy, Projects};

/// The manifest's caps (`docs/spec/extensions.md` › The manifest): a manifest is input like any
/// other, so everything is bounded.
const MAX_PROGRAMS: usize = 32;
const MAX_PROVIDES: usize = 16;
const MAX_PORTS: usize = 64;
const MAX_ID: usize = 64;
const MAX_NAME: usize = 128;
const MAX_CREDIT: usize = 512;
const MAX_PERMISSIONS: usize = 16;
/// Most a manifest file may be: far past any honest one.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
/// How deep a folder `extensions.add` copies; an honest extension is flat or nearly.
const MAX_COPY_DEPTH: usize = 8;

/// The runners Suhina ships, by a manifest's `runner` value.
const RUNNERS: &[&str] = &["lua"];

/// What an extension may do; a `files.write:` names the folder after the colon.
const PERMISSIONS: &[&str] = &["project.read", "project.write", "audio.read"];

/// One `provides` entry: what the extension offers, by id and label; a non-surface entry names
/// the command namespace it serves, a surface entry ([`is_surface`]) carries none.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Provides {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

impl Provides {
    /// Whether this entry marks a surface program ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md) › 10).
    pub fn is_surface(&self) -> bool {
        self.id == "surface"
    }
}

/// One `programs` entry: a file in the folder and its SHA-256.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Program {
    pub path: String,
    pub sha256: String,
}

/// An `extension.json` manifest, as it was read: every field checked, nothing yet trusted.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub provides: Vec<Provides>,
    pub license: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_checked: Option<String>,
    pub source: String,
    pub protocol: u64,
    #[serde(default)]
    pub programs: Vec<Program>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<String>,
}

impl Manifest {
    /// Reads and checks `folder/extension.json`; a refusal names the reason, and the extension
    /// is `refused` — no licence stated is a blocker, not a blank. The manifest comes back
    /// typed and as it was read, for `extensions.info` and the licence's name.
    pub fn read(folder: &Path) -> Result<(Self, Value), String> {
        let file = folder.join("extension.json");
        let meta = std::fs::metadata(&file).map_err(|_| "the manifest extension.json is missing".to_string())?;
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err(format!(
                "the manifest is {} bytes, over the {}",
                meta.len(),
                format_cap(MAX_MANIFEST_BYTES as usize)
            ));
        }
        let bytes = std::fs::read(&file).map_err(|e| format!("the manifest cannot be read: {e}"))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| format!("the manifest is not JSON: {e}"))?;
        let mut manifest: Manifest =
            serde_json::from_value(value.clone()).map_err(|e| format!("the manifest's fields do not fit: {e}"))?;
        manifest.check(&value, folder)?;
        Ok((manifest, value))
    }

    /// Every check the spec asks for, in the manifest's own terms; `value` is the parsed
    /// manifest for the licence object's fields.
    fn check(&mut self, value: &Value, folder: &Path) -> Result<(), String> {
        id(&self.id).map_err(|e| format!("id: {e}"))?;
        text(&self.name, MAX_NAME).map_err(|e| format!("name: {e}"))?;
        text(&self.version, MAX_NAME).map_err(|e| format!("version: {e}"))?;
        text(&self.source, MAX_NAME).map_err(|e| format!("source: {e}"))?;
        if self.provides.len() > MAX_PROVIDES {
            return Err(format!("provides: at most {MAX_PROVIDES} entries"));
        }
        for provides in &self.provides {
            id(&provides.id).map_err(|e| format!("provides: {e}"))?;
            text(&provides.label, MAX_NAME).map_err(|e| format!("provides: label: {e}"))?;
            match &provides.namespace {
                Some(namespace) if provides.is_surface() => {
                    return Err("provides: a surface entry carries no namespace".into());
                }
                Some(namespace) => {
                    id(namespace).map_err(|e| format!("provides: namespace: {e}"))?;
                }
                None if !provides.is_surface() => {
                    return Err(format!(
                        "provides {}: a namespace is required unless the entry is surface",
                        provides.id
                    ));
                }
                None => {}
            }
        }
        let surfaces = self.provides.iter().filter(|p| p.is_surface()).count();
        if surfaces > 1 {
            return Err("provides: at most one surface entry".into());
        }

        // The licence: an SPDX id, or a named file in the folder whose text is UTF-8.
        let licence =
            value.get("license").ok_or("license: a licence must be stated; none is a blocker, not a blank")?;
        match licence {
            Value::String(spdx) if !spdx.is_empty() => {}
            Value::Object(map) => {
                let name = map.get("name").and_then(Value::as_str).filter(|s| !s.is_empty());
                let text_file = map.get("text_file").and_then(Value::as_str).filter(|s| !s.is_empty());
                let (Some(name), Some(text_file)) = (name, text_file) else {
                    return Err("license: an object licence carries name and text_file".into());
                };
                text(name, MAX_NAME).map_err(|e| format!("license: name: {e}"))?;
                let path = relative(text_file).map_err(|e| format!("license: text_file: {e}"))?;
                let file = folder.join(&path);
                let bytes =
                    std::fs::read(&file).map_err(|_| format!("license: the text file {text_file} is missing"))?;
                if std::str::from_utf8(&bytes).is_err() {
                    return Err(format!("license: the text file {text_file} is not UTF-8 text"));
                }
            }
            _ => return Err("license: an SPDX id, or an object with name and text_file".into()),
        }
        if let Some(source) = &self.license_source {
            text(source, MAX_NAME * 4).map_err(|e| format!("license_source: {e}"))?;
        }
        if let Some(checked) = &self.license_checked {
            let date = checked.as_bytes();
            let ok = date.len() == 10
                && date[4] == b'-'
                && date[7] == b'-'
                && date.iter().enumerate().all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit());
            if !ok {
                return Err("license_checked: a date, YYYY-MM-DD".into());
            }
        }

        if self.programs.len() > MAX_PROGRAMS {
            return Err(format!("programs: at most {MAX_PROGRAMS}"));
        }
        for program in &self.programs {
            let path = relative(&program.path).map_err(|e| format!("programs: {e}"))?;
            let file = folder.join(&path);
            let bytes = std::fs::read(&file).map_err(|_| format!("programs: {} is missing", program.path))?;
            let digest = sha256(&bytes);
            if digest != program.sha256.to_ascii_lowercase() {
                return Err(format!(
                    "programs: {} has changed (its SHA-256 is {digest}, the manifest says {})",
                    program.path, program.sha256
                ));
            }
            if program.sha256.len() != 64 || !program.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("programs: {}'s sha256 is 64 hex digits", program.path));
            }
        }

        if let Some(ports) = &self.ports {
            if ports.len() > MAX_PORTS {
                return Err(format!("ports: at most {MAX_PORTS} patterns"));
            }
            for pattern in ports {
                text(pattern, crate::extensions::programs::MAX_PORT_NAME).map_err(|e| format!("ports: {e}"))?;
            }
        }
        if let Some(runner) = &self.runner {
            if !RUNNERS.contains(&runner.as_str()) {
                return Err(format!(
                    "runner: {runner:?} is not one Suhina ships (it ships {})",
                    RUNNERS.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>().join(", ")
                ));
            }
            // A runner's extension is a surface program: its manifest claims ports and names
            // the one script the runner runs.
            if self.ports.is_none() {
                return Err("ports: a surface program's manifest claims the ports it uses".into());
            }
            if self.programs.len() != 1 {
                return Err(format!(
                    "programs: a runner's manifest names the one script it runs (it names {})",
                    self.programs.len()
                ));
            }
            if self.protocol > crate::extensions::programs::PROTOCOL {
                return Err(format!(
                    "protocol: {} is newer than the {} this Suhina speaks",
                    self.protocol,
                    crate::extensions::programs::PROTOCOL
                ));
            }
        }
        if self.protocol == 0 {
            return Err("protocol: the host protocol version it speaks, from 1".into());
        }

        if self.permissions.len() > MAX_PERMISSIONS {
            return Err(format!("permissions: at most {MAX_PERMISSIONS}"));
        }
        for permission in &self.permissions {
            let known = PERMISSIONS.contains(&permission.as_str())
                || permission.strip_prefix("files.write:").is_some_and(|folder| !folder.is_empty());
            if !known {
                return Err(format!("permissions: {permission:?} is not one Suhina knows"));
            }
        }
        if let Some(credit) = &self.credit {
            text(credit, MAX_CREDIT).map_err(|e| format!("credit: {e}"))?;
        }
        Ok(())
    }

    /// The script file a runner runs: the first `programs` entry, relative to the folder.
    pub fn script(&self) -> Option<&str> {
        self.programs.first().map(|p| p.path.as_str())
    }

    /// The licence's name for a list: the SPDX id, or the object's name.
    pub fn licence_name(&self, value: &Value) -> String {
        match value.get("license") {
            Some(Value::String(spdx)) => spdx.clone(),
            Some(Value::Object(map)) => map.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            _ => String::new(),
        }
    }
}

/// A known extension: its folder in the store, what its manifest says, and where it stands.
pub struct Extension {
    pub id: String,
    pub folder: PathBuf,
    pub manifest: Option<Manifest>,
    /// The manifest as it was read, for `extensions.info` and the licence's name.
    pub value: Option<Value>,
    /// Why it is not usable, when it is not.
    pub reason: Option<String>,
    pub state: State,
    /// The last error a running program reported, cleared by `extensions.restart`.
    pub last_error: Option<String>,
    /// The program's handle, when it has been started.
    pub program: Option<programs::Handle>,
}

/// Where an extension stands (`docs/spec/extensions.md` › Terms). A folder whose manifest is
/// gone is `refused` with the reason naming it; a folder that vanished from the store between
/// re-reads is simply not in the store, so there is no `missing` state to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Available,
    Running,
    Refused,
    Crashed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Running => "running",
            Self::Refused => "refused",
            Self::Crashed => "crashed",
        }
    }
}

/// The settings folder's `extensions/`, by the system's settings folder — the store the CLI
/// and the MCP server read, which never name a root of their own.
fn default_root() -> Option<PathBuf> {
    suhina_plugins::library::settings_folder().map(|settings| settings.join("extensions"))
}

/// The extensions Suhina knows: the folders of the store, each read once, plus the programs
/// running. The root is `SUHINA_EXTENSIONS` when it names one; otherwise the caller's settings
/// folder's `extensions/` ([`Self::set_root`]), and until then the system's settings folder's.
pub struct Extensions {
    /// The root `SUHINA_EXTENSIONS` named, which wins over every caller's folder.
    env_root: Option<PathBuf>,
    root: Option<PathBuf>,
    known: Vec<Extension>,
}

impl Default for Extensions {
    fn default() -> Self {
        Self::new()
    }
}

impl Extensions {
    pub fn new() -> Self {
        let env_root = std::env::var_os("SUHINA_EXTENSIONS").map(PathBuf::from);
        let mut extensions =
            Self { env_root: env_root.clone(), root: env_root.or_else(default_root), known: Vec::new() };
        extensions.reload();
        extensions
    }

    /// Points the store at `root` — the caller's settings folder's `extensions/` — unless
    /// `SUHINA_EXTENSIONS` named a root, which wins over every caller. The desktop app and the
    /// MCP server give their settings folder at start-up; the CLI relies on [`Self::new`]'s
    /// default. See [`Self::force_root`] for the tests' say-so-over-everything root.
    pub fn set_root(&mut self, root: &Path) {
        if self.env_root.is_some() {
            return;
        }
        self.root = Some(root.to_path_buf());
        self.reload();
    }

    /// Forces the root over `SUHINA_EXTENSIONS` too, so a test's store is its own whatever the
    /// developer's environment names.
    pub fn force_root(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
        self.reload();
    }

    /// The store's root, when there is one.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Reads the store's folders again: what the user (or a test) put there is picked up. The
    /// programs running keep running — their handles re-attach to the re-read entries by id,
    /// and a program whose extension is gone from the store stops when its handle is dropped.
    pub fn reload(&mut self) {
        let mut running: Vec<(String, programs::Handle)> = self
            .known
            .iter_mut()
            .filter_map(|extension| extension.program.take().map(|program| (extension.id.clone(), program)))
            .collect();
        self.known.clear();
        let Some(root) = &self.root else { return };
        let Ok(entries) = std::fs::read_dir(root) else { return };
        let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        folders.sort();
        for folder in folders {
            if !folder.is_dir() {
                continue;
            }
            let Some(id) = folder.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
            let mut known = match Manifest::read(&folder) {
                Ok((manifest, value)) => Extension {
                    id,
                    folder,
                    manifest: Some(manifest),
                    value: Some(value),
                    reason: None,
                    state: State::Available,
                    last_error: None,
                    program: None,
                },
                Err(reason) => Extension {
                    id,
                    folder,
                    manifest: None,
                    value: None,
                    reason: Some(reason),
                    state: State::Refused,
                    last_error: None,
                    program: None,
                },
            };
            if let Some(at) = running.iter().position(|(running_id, _)| *running_id == known.id) {
                let (_, program) = running.remove(at);
                known.program = Some(program);
                known.state = State::Running;
            }
            self.known.push(known);
        }
        // A program whose extension is gone from the store: dropped here, so it stops.
    }

    fn known_mut(&mut self, id: &str) -> Result<&mut Extension, CommandError> {
        self.known
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("no extension {id:?}; see extensions.list").into())
    }

    /// Drains every running program's events, applies them to the states, and returns what
    /// came, paired with the extension's id. The one drain point: the session's frame calls
    /// [`Self::accept_events`], and the tests read the events through the same drain.
    pub fn drain_events(&mut self) -> Vec<(String, programs::Event)> {
        let mut drained = Vec::new();
        for extension in &mut self.known {
            let Some(program) = &mut extension.program else { continue };
            for event in program.drain() {
                match &event {
                    programs::Event::Running => extension.state = State::Running,
                    programs::Event::Crashed(reason) => {
                        extension.state = State::Crashed;
                        extension.last_error = Some(reason.clone());
                    }
                    programs::Event::ScriptError(message) => extension.last_error = Some(message.clone()),
                    programs::Event::SendBack { .. } => {
                        // The MIDI slice consumes these; until then they are counted, not kept.
                    }
                    programs::Event::Dropped(count) => {
                        extension.last_error = Some(format!("{count} send-backs dropped, the queue was full"));
                    }
                }
                drained.push((extension.id.clone(), event));
            }
        }
        drained
    }

    /// What the supervisor reported, applied to the states; called where the session drains
    /// its queues.
    pub fn accept_events(&mut self) {
        let _ = self.drain_events();
    }

    /// Blocks until a running program's queue holds an event that answers `which`, then drains
    /// — applying the states — and returns the first matching event, with its extension's id.
    /// A wait on the supervisor's signal, not a clock (AGENTS.md rule 7). The tests start
    /// exactly one program, so the one queue is the whole story; the session's frame never
    /// waits ([`Self::accept_events`]).
    pub fn wait_for_event(&mut self, which: impl Fn(&programs::Event) -> bool) -> Option<(String, programs::Event)> {
        let shared =
            self.known.iter().find_map(|extension| extension.program.as_ref()).map(|program| program.shared())?;
        let (queue, signal) = &*shared;
        let mut queue = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while !queue.iter().any(&which) {
            queue = signal.wait(queue).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(queue);
        self.drain_events().into_iter().find(|(_, event)| which(event))
    }

    /// The running program of an extension, by id: the MIDI slice sends it the ports and takes
    /// its send-backs.
    pub fn program(&self, id: &str) -> Option<&programs::Handle> {
        self.known.iter().find(|e| e.id == id).and_then(|e| e.program.as_ref())
    }

    /// The running program of an extension, by id, for changing.
    pub fn program_mut(&mut self, id: &str) -> Option<&mut programs::Handle> {
        self.known.iter_mut().find(|e| e.id == id).and_then(|e| e.program.as_mut())
    }

    /// The one program running, for the tests: this slice's store starts none by itself, so
    /// the tests start exactly one.
    pub fn programs(&self) -> Option<&programs::Handle> {
        self.known.iter().find_map(|e| e.program.as_ref())
    }
}

/// `extensions.list`: every known extension with its state and, when it is not usable, why.
fn list(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    projects.extensions.accept_events();
    let list: Vec<Value> = projects
        .extensions
        .known
        .iter()
        .map(|e| {
            let manifest = e.manifest.as_ref();
            json!({
                "id": e.id,
                "name": manifest.map(|m| m.name.clone()),
                "version": manifest.map(|m| m.version.clone()),
                "provides": manifest.map(|m| m.provides.iter().map(|p| p.id.clone()).collect::<Vec<_>>()),
                "state": e.state.name(),
                "license": manifest.map(|m| m.licence_name(e.value.as_ref().unwrap_or(&Value::Null))),
                "source": manifest.map(|m| m.source.clone()),
                "reason": e.reason,
                "lastError": e.last_error,
            })
        })
        .collect();
    Ok(json!({ "extensions": list }))
}

/// `extensions.add`: checks `folder`'s manifest, copies the folder into the store under the
/// manifest's id, and answers the licence and the permissions. Not an undo step.
fn add(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let Some(root) = projects.extensions.root.clone() else {
        return Err("there is no extensions folder: no settings folder is set, and SUHINA_EXTENSIONS is not".into());
    };
    let folder = Args::new(params).text("folder")?;
    let source = Path::new(folder);
    if !source.is_dir() {
        return Err(format!("{} is not a folder", source.display()).into());
    }
    let (manifest, value) = Manifest::read(source).map_err(CommandError::from)?;
    let destination = root.join(&manifest.id);
    if destination.exists() {
        return Err(format!("an extension {:?} is already in the store; remove it first", manifest.id).into());
    }
    copy_folder(source, &destination, 0)?;
    projects.extensions.reload();
    if let Some(reason) = &projects.extensions.known_mut(&manifest.id)?.reason {
        let _ = std::fs::remove_dir_all(&destination);
        return Err(reason.clone().into());
    }
    let licence = manifest.licence_name(&value);
    Ok(json!({
        "id": manifest.id,
        "license": licence,
        "permissions": manifest.permissions,
        "folder": destination.display().to_string(),
    }))
}

/// `extensions.info`: one extension in full — the manifest, the licence text, the permissions,
/// the state and the last error.
fn info(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    projects.extensions.accept_events();
    let id = Args::new(params).text("id")?;
    let extension = projects.extensions.known_mut(id)?;
    let manifest = extension.manifest.as_ref();
    let licence_text = manifest.and_then(|m| match &m.license {
        Value::String(spdx) => Some(spdx.clone()),
        Value::Object(_) => {
            let file = extension
                .value
                .as_ref()
                .and_then(|v| v.get("license"))
                .and_then(|l| l.get("text_file"))
                .and_then(Value::as_str);
            file.and_then(|file| std::fs::read_to_string(extension.folder.join(file)).ok())
        }
        _ => None,
    });
    Ok(json!({
        "id": extension.id,
        "manifest": extension.value,
        "licenseText": licence_text,
        "permissions": manifest.map(|m| m.permissions.clone()),
        "state": extension.state.name(),
        "reason": extension.reason,
        "lastError": extension.last_error,
    }))
}

/// `extensions.restart`: stops the program that is running and starts it again on what the
/// folder holds now — the manifest is read again, so the restart runs a script whose SHA-256
/// was checked this run. A refused or missing extension is refused with its reason.
fn restart(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = Args::new(params).text("id")?;
    projects.extensions.accept_events();
    let folder = projects.extensions.known_mut(id)?.folder.clone();
    // The old program stops before the new one takes its ports.
    let old = projects.extensions.known_mut(id)?.program.take();
    if let Some(old) = old {
        old.stop();
    }
    let (manifest, value) = Manifest::read(&folder).map_err(CommandError::from)?;
    let Some(runner) = &manifest.runner else {
        return Err(format!("{id:?} is not a surface program; there is nothing to restart").into());
    };
    let Some(script) = manifest.script() else {
        return Err(format!("{id:?}'s manifest names no program file").into());
    };
    let handle = programs::start(id, runner, &folder, script)?;
    let extension = projects.extensions.known_mut(id)?;
    extension.manifest = Some(manifest);
    extension.value = Some(value);
    extension.reason = None;
    extension.last_error = None;
    extension.program = Some(handle);
    extension.state = State::Running;
    Ok(json!({ "id": id, "state": "running" }))
}

/// `extensions.remove`: forgets an extension; its program stops first, the user's own folder
/// stays where it is, and the store's copy goes.
fn remove(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = Args::new(params).text("id")?;
    projects.extensions.accept_events();
    let (program, folder) = {
        let extension = projects.extensions.known_mut(id)?;
        (extension.program.take(), extension.folder.clone())
    };
    if let Some(program) = program {
        program.stop();
    }
    if let Some(root) = &projects.extensions.root
        && folder.starts_with(root)
    {
        let _ = std::fs::remove_dir_all(&folder);
    }
    projects.extensions.reload();
    Ok(json!({ "id": id, "removed": true }))
}

const FOLDER: crate::commands::Param =
    param("folder", ParamType::Path(crate::PathKind::Folder), "the extension's folder, with extension.json in it");
const ID: crate::commands::Param = param("id", ParamType::Text, "an extension id, from extensions.list");

/// The `extensions.*` commands, on [`Projects`] like the `plugin.*` ones: they belong to Suhina,
/// not to one project.
pub fn extensions_commands() -> &'static [CommandSpec<Projects>] {
    static COMMANDS: std::sync::OnceLock<Vec<CommandSpec<Projects>>> = std::sync::OnceLock::new();
    COMMANDS.get_or_init(|| {
        vec![
            CommandSpec {
                id: "extensions.list",
                label: "List every known extension: id, name, version, what it provides, its state, its licence's name and source, and the reason when it is not usable",
                params: &[],
                kind: CommandKind::on(crate::commands::Target::Projects, crate::commands::Effect::Read),
                run: list,
                policy: Policy::DEFAULT,
            },
            CommandSpec {
                id: "extensions.add",
                label: "Add an extension from a folder: checks its manifest and licence, copies the folder into the extensions store under the manifest's id, and answers the licence and the permissions it declares; not an undo step",
                params: &[FOLDER],
                kind: CommandKind::on(crate::commands::Target::Projects, crate::commands::Effect::Change),
                run: add,
                policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
            },
            CommandSpec {
                id: "extensions.info",
                label: "One extension in full: its manifest, its licence's text, the permissions it declares, its state and its last error",
                params: &[ID],
                kind: CommandKind::on(crate::commands::Target::Projects, crate::commands::Effect::Read),
                run: info,
                policy: Policy::DEFAULT,
            },
            CommandSpec {
                id: "extensions.restart",
                label: "Stop a surface program and start it again: a stopped, crashed or wedged one comes back, and its ports go back to it",
                params: &[ID],
                kind: CommandKind::on(crate::commands::Target::Projects, crate::commands::Effect::Change),
                run: restart,
                policy: Policy::DEFAULT,
            },
            CommandSpec {
                id: "extensions.remove",
                label: "Forget an extension: its program stops, the store's copy of its folder goes, and the folder you added stays where it is",
                params: &[ID],
                kind: CommandKind::on(crate::commands::Target::Projects, crate::commands::Effect::Change),
                run: remove,
                policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
            },
        ]
    })
}

/// A manifest id: `[a-z0-9]` then `[a-z0-9._-]`, at most 64 characters.
fn id(text: &str) -> Result<(), String> {
    let first = text.bytes().next().ok_or("it is empty")?;
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let rest = |b: u8| ok(b) || matches!(b, b'.' | b'_' | b'-');
    if !ok(first) || !text.bytes().skip(1).all(rest) {
        return Err("it is lowercase letters, digits and . _ -, starting with a letter or digit".into());
    }
    if text.len() > MAX_ID {
        return Err(format!("it is over {MAX_ID} characters"));
    }
    Ok(())
}

/// Non-empty text within a cap.
fn text(text: &str, cap: usize) -> Result<(), String> {
    if text.is_empty() {
        Err("it is empty".into())
    } else if text.len() > cap {
        Err(format!("it is over {}", format_cap(cap)))
    } else {
        Ok(())
    }
}

fn format_cap(cap: usize) -> String {
    if cap >= 1024 { format!("{} KiB cap", cap / 1024) } else { format!("{cap} characters") }
}

/// A path a manifest names, relative to the folder and never out of it.
fn relative(path: &str) -> Result<PathBuf, String> {
    text(path, MAX_NAME * 4)?;
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(format!("{path:?} is absolute; the folder's paths are relative"));
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            _ => return Err(format!("{path:?} leaves the extension's folder")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("it is empty".into());
    }
    Ok(out)
}

/// A folder copied whole: files and folders, within a depth, nothing followed out of it.
fn copy_folder(from: &Path, to: &Path, depth: usize) -> Result<(), CommandError> {
    if depth > MAX_COPY_DEPTH {
        return Err(format!("the folder is nested deeper than {MAX_COPY_DEPTH} levels").into());
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{} cannot be made: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{} cannot be read: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("{} cannot be read: {e}", from.display()))?;
        let kind = entry.file_type().map_err(|e| format!("{} cannot be read: {e}", entry.path().display()))?;
        let to = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &to, depth + 1)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &to)
                .map_err(|e| format!("{} cannot be copied: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// SHA-256 of a file's bytes, as the manifest's lowercase hex.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
