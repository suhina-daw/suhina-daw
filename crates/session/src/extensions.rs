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
use crate::extensions::programs::Shared;
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
/// the command namespace it serves, a surface entry ([`Provides::is_surface`]) carries none.
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
                let bytes = capped(&file, MAX_LICENCE)
                    .map_err(|reason| format!("license: the text file {text_file} {reason}"))?;
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
            let digest =
                file_digest(&file, MAX_SCRIPT).map_err(|reason| format!("programs: {}: {reason}", program.path))?;
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
    /// The script stopped at its error boundary: the process keeps running, but the script is
    /// dead until `extensions.restart`, so the program holds no ports meanwhile.
    script_stopped: bool,
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

/// A surface program the scan would start now, with what its first process hears at its
/// handshake: the ports its patterns claim among the ones present, minus what the running
/// programs took first.
struct Candidate {
    id: String,
    folder: PathBuf,
    runner: String,
    script: String,
    ports: (Vec<String>, Vec<String>),
}

/// The extensions Suhina knows: the folders of the store, each read once, plus the programs
/// running. The root is `SUHINA_EXTENSIONS` when it names one; otherwise the caller's settings
/// folder's `extensions/` ([`Self::set_root`]), and until then the system's settings folder's.
pub struct Extensions {
    /// The root `SUHINA_EXTENSIONS` named, which wins over every caller's folder.
    env_root: Option<PathBuf>,
    root: Option<PathBuf>,
    known: Vec<Extension>,
    /// Bumped whenever the programs running (and so the ports claimed) may have changed: a
    /// program started or stopped, its script stopped at its boundary, the store re-read. The
    /// MIDI router compares it against the look it last synced at.
    pub(crate) claims_epoch: u64,
    /// The live subscriptions of the running programs, re-read each look.
    subs: Vec<Sub>,
    /// The extensions a scan's auto-start failed for, so the scan does not try the same
    /// start every two seconds; cleared when the store re-reads.
    auto_blocked: std::collections::BTreeSet<String>,
}

/// A live subscription: the program that made it, its token, the reading command and its
/// parameters, and the value last pushed — none yet, for a fresh one.
struct Sub {
    program: String,
    /// The script's own token, echoed back on every push.
    token: u64,
    /// The program handle's generation the subscribe came from: a restart's fresh script
    /// subscribes with the same script tokens, and the old handle's records would swallow
    /// its first push (their `last` already holds the value) if they lived on.
    handle: u64,
    cmd: String,
    params: Value,
    last: Option<Value>,
}

impl Default for Extensions {
    fn default() -> Self {
        Self::new()
    }
}

impl Extensions {
    pub fn new() -> Self {
        let env_root = std::env::var_os("SUHINA_EXTENSIONS").map(PathBuf::from);
        let mut extensions = Self {
            env_root: env_root.clone(),
            root: env_root.or_else(default_root),
            known: Vec::new(),
            claims_epoch: 1,
            subs: Vec::new(),
            auto_blocked: std::collections::BTreeSet::new(),
        };
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
    /// programs running keep running when the folder still holds the manifest they were
    /// started from — their handles re-attach to the re-read entries by id, and a program
    /// whose extension is gone from the store, or whose manifest changed under it, stops
    /// when its handle is dropped.
    pub fn reload(&mut self) {
        self.auto_blocked.clear();
        let mut running: Vec<(String, bool, Option<Value>, programs::Handle)> =
            self.known
                .iter_mut()
                .filter_map(|extension| {
                    extension.program.take().map(|program| {
                        (extension.id.clone(), extension.script_stopped, extension.value.clone(), program)
                    })
                })
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
            // A hidden folder is not an extension's: an add's staging folder (a copy that
            // never finished) keeps out of the list.
            if folder.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')) {
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
                    script_stopped: false,
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
                    script_stopped: false,
                    program: None,
                },
            };
            if let Some(at) = running.iter().position(|(running_id, ..)| *running_id == known.id) {
                let (_, stopped, old_value, program) = running.remove(at);
                // The process keeps running only on what it was started from: the same
                // manifest as the one read now. A changed one — the script's checksum
                // moved, the claims or the runner changed — is not what the process runs:
                // it stops, and the store says what the folder holds now, refused or not.
                let still_matches = matches!((&known.value, &old_value), (Some(new), Some(old)) if new == old);
                if still_matches {
                    known.program = Some(program);
                    known.state = State::Running;
                    known.script_stopped = stopped;
                } else {
                    program.stop();
                }
            }
            self.known.push(known);
        }
        // A program whose extension is gone from the store: dropped here, so it stops. Either
        // way the claims may have changed, so the router looks again.
        self.claims_epoch += 1;
    }

    fn known_mut(&mut self, id: &str) -> Result<&mut Extension, CommandError> {
        self.known
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("no extension {id:?}; see extensions.list").into())
    }

    /// Drains every running program's events, applies them to the states, takes the
    /// subscriptions in and out, and returns what came, paired with the extension's id — the
    /// send-backs and the scripts' commands and reads for the session to service. The one
    /// drain point: the session's frame calls [`Projects::poll_programs`], and the tests read
    /// the events through the same drain.
    pub fn drain_events(&mut self) -> Vec<(String, programs::Event)> {
        let mut drained = Vec::new();
        let mut subscribing = Vec::new();
        let mut unsubscribed = Vec::new();
        for extension in &mut self.known {
            let Some(program) = &mut extension.program else { continue };
            for event in program.drain() {
                match &event {
                    programs::Event::Running => {
                        extension.state = State::Running;
                        extension.script_stopped = false;
                        // The claims may have changed: the router looks again. The process's
                        // subscriptions are not dropped here: the queue's order puts them
                        // ahead of Running (the chunk subscribes before `ready`), and a drop
                        // would wipe the fresh ones every look, leaving the script blind.
                        self.claims_epoch += 1;
                    }
                    programs::Event::Crashed(reason) => {
                        extension.state = State::Crashed;
                        extension.last_error = Some(reason.clone());
                        // The dead process's subscriptions die with it: a fresh script
                        // subscribes anew, and the old records would swallow its first push
                        // — their `last` already holds the value, so while nothing changed
                        // they push nothing, leaving the new script's reads nil.
                        self.subs.retain(|sub| sub.program != extension.id);
                        self.claims_epoch += 1;
                    }
                    programs::Event::ScriptError(message) => {
                        extension.last_error = Some(message.clone());
                        // The script is dead until `extensions.restart`; its ports fall back
                        // (docs/spec/midi-mapping.md › Surface programs). Its subscriptions
                        // die with it: the restart's fresh script subscribes anew.
                        extension.script_stopped = true;
                        self.subs.retain(|sub| sub.program != extension.id);
                        self.claims_epoch += 1;
                    }
                    programs::Event::SendBack { .. } => {
                        // The MIDI router writes these out, where the frame drains.
                    }
                    programs::Event::Dropped(count) => {
                        extension.last_error = Some(format!("{count} send-backs dropped, the queue was full"));
                    }
                    // The session runs these and answers; they pass through.
                    programs::Event::Command { .. } | programs::Event::Read { .. } => {}
                    // A subscription the store keeps; a live one the store takes out.
                    programs::Event::Subscribe { token, id, params } => {
                        subscribing.push((extension.id.clone(), *token, id.clone(), params.clone()));
                        continue;
                    }
                    programs::Event::Unsubscribe { token } => {
                        unsubscribed.push((extension.id.clone(), *token));
                        continue;
                    }
                }
                drained.push((extension.id.clone(), event));
            }
        }
        for (program, token, cmd, params) in subscribing {
            // The subscribe binds to the handle that is running now; an older generation's
            // records go before the count — a restart's fresh script re-subscribes with the
            // same script tokens, and the old `last` values would swallow its first push
            // (the controller dark) until something moved. Their number must not fill the
            // cap the fresh ones are counted against, or a full old script would leave the
            // new one no subscription.
            let handle = self.program(&program).map(|h| h.token()).unwrap_or(0);
            self.subs.retain(|sub| !(sub.program == program && sub.handle != handle));
            // At most the protocol's 64 live subscriptions per program; the runner refuses a
            // subscribe past it, so this one only guards what crosses the wire.
            let live = self.subs.iter().filter(|sub| sub.program == program).count();
            if live < programs::MAX_SUBSCRIPTIONS {
                self.subs.push(Sub { program, token, handle, cmd, params, last: None });
            }
        }
        for (program, token) in unsubscribed {
            self.subs.retain(|sub| !(sub.program == program && sub.token == token));
        }
        // A program whose handle is gone — stopped, crashed past its restarts, removed — takes
        // its subscriptions with it.
        self.subs.retain(|sub| self.known.iter().any(|e| e.id == sub.program && e.program.is_some()));
        drained
    }

    /// What each subscription wants re-read this look: (program, token, command, parameters).
    pub(crate) fn sub_reads(&self) -> Vec<(String, u64, String, Value)> {
        self.subs.iter().map(|sub| (sub.program.clone(), sub.token, sub.cmd.clone(), sub.params.clone())).collect()
    }

    /// Takes the reads' results in, and says what to push: a `state` for every subscription
    /// whose value changed from the last one sent — the first read after a subscribe always
    /// has. A refused read pushes nothing; the next look reads again.
    pub(crate) fn apply_sub_results(
        &mut self,
        results: Vec<(String, u64, Result<Value, String>)>,
    ) -> Vec<(String, u64, Value)> {
        let mut pushed = Vec::new();
        for (program, token, result) in results {
            let Ok(value) = result else {
                continue;
            };
            let Some(sub) = self.subs.iter_mut().find(|sub| sub.program == program && sub.token == token) else {
                continue;
            };
            if sub.last.as_ref() != Some(&value) {
                sub.last = Some(value.clone());
                pushed.push((sub.program.clone(), sub.token, value));
            }
        }
        pushed
    }

    /// Blocks until a running program's queue holds an event that answers `which`, then drains
    /// — applying the states — and returns the first matching event, with its extension's id.
    /// A wait on the supervisor's signal, not a clock (AGENTS.md rule 7). The tests start
    /// exactly one program, so the one queue is the whole story; the session's frame never
    /// waits ([`Projects::poll_programs`]). The answer is the first event that answers
    /// `which`; the events reported after it stay queued, so a wait for a crash does not
    /// swallow the replacement's `Running`. The error is the terminal reason: the
    /// supervisor is over and no event can come.
    pub fn wait_for_event(
        &mut self,
        which: impl Fn(&programs::Event) -> bool,
    ) -> Result<(String, programs::Event), String> {
        let Some(shared) =
            self.known.iter().find_map(|extension| extension.program.as_ref()).map(|program| program.shared())
        else {
            return Err("no program runs".into());
        };
        let mut queue = shared.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if queue.iter().any(&which) {
                break;
            }
            if let Some(reason) = shared.terminal.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
                return Err(reason);
            }
            queue = shared.signal.wait(queue).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(queue);
        let mut drained = self.drain_events();
        let Some(at) = drained.iter().position(|(_, event)| which(event)) else {
            return Err("the event answered, and the drain took it elsewhere".into());
        };
        // The state effects of everything drained stay applied (they are idempotent);
        // the events after the answer are the news that is still owed.
        let owed: Vec<(String, programs::Event)> = drained.split_off(at + 1);
        for (id, event) in owed.iter().rev() {
            if let Some(program) = self.program_mut(id) {
                program.requeue(event.clone());
            }
        }
        let (id, event) = drained.remove(at);
        Ok((id, event))
    }

    /// The programs' shared event queue and its signal, for the tests' pump-while-waiting
    /// (the fixture's `pump_until`, around [`Projects::poll_plugins`]): the first running
    /// program's, or none while no program runs.
    pub(crate) fn event_queue(&self) -> Option<Shared> {
        self.known.iter().filter_map(|extension| extension.program.as_ref()).map(|program| program.shared()).next()
    }

    /// Whether the queue already holds an event that answers `which`, without waiting.
    /// Blocks until a running program's queue holds an event that answers `which`, and leaves
    /// it queued: the router's tests wait for a send-back this way, then let the frame's drain
    /// — the one that writes it out — take it. On the supervisor's signal, not a clock. The
    /// error is the terminal reason: the supervisor is over and no event can come.
    pub fn peek_for_event(&mut self, which: impl Fn(&programs::Event) -> bool) -> Result<(), String> {
        let Some(shared) =
            self.known.iter().find_map(|extension| extension.program.as_ref()).map(|program| program.shared())
        else {
            return Err("no program runs".into());
        };
        let mut queue = shared.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if queue.iter().any(&which) {
                return Ok(());
            }
            if let Some(reason) = shared.terminal.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
                return Err(reason);
            }
            queue = shared.signal.wait(queue).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
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

    /// Starts the surface programs whose device is here: an installed, validated extension
    /// that is not running starts when the scan found a port its manifest's `ports` patterns
    /// match — at launch with the controller already connected, and later when it is plugged
    /// in (docs/spec/extensions.md › Running one). A crashed or stopped-at-its-boundary
    /// program is not the scan's to start again: the supervisor restarts the first within
    /// its window, and `extensions.restart` answers the second. The ids started, so the
    /// caller knows the claims moved; a start that fails is said once on the extension and
    /// not retried by the scan until the store re-reads.
    pub(crate) fn start_for_ports(&mut self, inputs: &[String], outputs: &[String]) -> Vec<String> {
        // One pass in the store's order, the way the router's first take goes: a running
        // program's patterns take their ports first, and what is left that matches a
        // candidate's patterns is the snapshot its first process hears at its handshake —
        // a controller's script starts already knowing its ports' names, so a chunk that
        // sends sees its outputs; with no port present the snapshot is empty and no start
        // happens. The starts are applied by id after, since starting one moves `self`.
        let mut taken: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut candidates: Vec<Candidate> = Vec::new();
        for extension in &self.known {
            // A running program's patterns take their ports first, the way the router's
            // first take goes; a crashed or refused one holds none (the router released
            // them), so it takes nothing here either.
            if extension.state == State::Running && !extension.script_stopped {
                if let Some(patterns) = extension.manifest.as_ref().and_then(|manifest| manifest.ports.as_ref()) {
                    let held: Vec<String> = inputs
                        .iter()
                        .chain(outputs.iter())
                        .filter(|name| !taken.contains(*name) && patterns.iter().any(|pat| port_matches(pat, name)))
                        .cloned()
                        .collect();
                    taken.extend(held);
                }
                continue;
            }
            if extension.state != State::Available || self.auto_blocked.contains(&extension.id) {
                continue;
            }
            let Some(manifest) = extension.manifest.as_ref() else { continue };
            let Some(patterns) = manifest.ports.as_ref() else { continue };
            let free = |name: &&String| !taken.contains(*name) && patterns.iter().any(|pat| port_matches(pat, name));
            let claim_inputs: Vec<String> = inputs.iter().filter(free).cloned().collect();
            let claim_outputs: Vec<String> = outputs.iter().filter(free).cloned().collect();
            taken.extend(claim_inputs.iter().cloned());
            taken.extend(claim_outputs.iter().cloned());
            let Some(runner) = manifest.runner.as_ref() else { continue };
            let Some(script) = manifest.script().map(str::to_string) else { continue };
            candidates.push(Candidate {
                id: extension.id.clone(),
                folder: extension.folder.clone(),
                runner: runner.clone(),
                script,
                ports: (claim_inputs, claim_outputs),
            });
        }
        let mut started = Vec::new();
        for candidate in candidates {
            if candidate.ports.0.is_empty() && candidate.ports.1.is_empty() {
                continue;
            }
            match programs::start(
                &candidate.id,
                &candidate.runner,
                &candidate.folder,
                &candidate.script,
                candidate.ports,
            ) {
                Ok(handle) => {
                    if let Some(known) = self.known.iter_mut().find(|known| known.id == candidate.id) {
                        known.program = Some(handle);
                        known.state = State::Running;
                        known.last_error = None;
                    }
                    started.push(candidate.id);
                }
                Err(reason) => {
                    if let Some(known) = self.known.iter_mut().find(|known| known.id == candidate.id) {
                        known.last_error = Some(reason.to_string());
                    }
                    self.auto_blocked.insert(candidate.id);
                }
            }
        }
        if !started.is_empty() {
            self.claims_epoch += 1;
        }
        started
    }

    /// Records a start's failure on the extension: the process is not running (the store
    /// says what the folder holds now — a validated manifest, if one was read), its
    /// subscriptions go with the process that is gone, and the router looks again, for the
    /// claims have moved. The user's next restart, or the scan's next look, starts it again.
    pub(crate) fn start_failed(&mut self, id: &str, reason: String, manifest: Manifest, value: Value) {
        if let Ok(extension) = self.known_mut(id) {
            extension.manifest = Some(manifest);
            extension.value = Some(value);
            extension.reason = None;
            extension.last_error = Some(reason);
            extension.script_stopped = false;
            extension.program = None;
            extension.state = State::Available;
        }
        self.subs.retain(|sub| sub.program != id);
        self.claims_epoch += 1;
    }

    /// What each running program claims, for the MIDI router: the extension's id, its
    /// manifest's port name patterns, and the handle to deliver to. A program whose script
    /// stopped at its boundary holds no ports (they fall back) until `extensions.restart`.
    pub(crate) fn claims(&self) -> Vec<Claim<'_>> {
        self.known
            .iter()
            .filter_map(|extension| {
                if extension.state != State::Running || extension.script_stopped {
                    return None;
                }
                let handle = extension.program.as_ref()?;
                let patterns = extension.manifest.as_ref()?.ports.as_deref()?;
                Some(Claim { id: &extension.id, patterns, handle })
            })
            .collect()
    }

    /// The one program running, for the tests: the store starts programs itself when a
    /// device for one is present, so a test that wants a specific one started still starts
    /// it by hand.
    pub fn programs(&self) -> Option<&programs::Handle> {
        self.known.iter().find_map(|e| e.program.as_ref())
    }
}

/// What a running surface program claims, for one look at the ports: its extension's id, the
/// manifest's port name patterns, and the handle the router delivers to.
pub(crate) struct Claim<'a> {
    pub(crate) id: &'a str,
    pub(crate) patterns: &'a [String],
    pub(crate) handle: &'a programs::Handle,
}

/// Whether a port name matches a manifest pattern: `*` wildcards, case ignored
/// (`docs/spec/extensions.md` › The manifest, the way a controller profile's patterns match).
pub(crate) fn port_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    // Between the wildcards, each piece must be found whole, in order: the first as a prefix,
    // the last as a suffix, the middles anywhere but not before the piece before them.
    let mut rest = name.as_str();
    let pieces: Vec<&str> = pattern.split('*').collect();
    let Some((first, tail)) = pieces.split_first() else { return false };
    let Some((last, middles)) = tail.split_last() else {
        // No wildcard at all: the whole name must be the pattern.
        return rest == *first;
    };
    let Some(after_first) = rest.strip_prefix(first) else { return false };
    rest = after_first;
    for middle in middles {
        match rest.find(middle) {
            Some(at) => rest = &rest[at + middle.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// `extensions.list`: every known extension with its state and, when it is not usable, why.
fn list(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    // The pending requests the programs made are serviced here too, not dropped.
    projects.poll_programs();
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
    // The copy is made in a staging folder and published only when it is whole: a failure
    // leaves the destination clear, so the user fixes the source and adds it again — the
    // store never holds half of one.
    let staging = root.join(format!(".add-{}.staging", manifest.id));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(reason) = copy_folder(source, &staging, 0) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(reason);
    }
    // The copy is read before it is published: what arrived is what was validated.
    if let Err(reason) = Manifest::read(&staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(reason.into());
    }
    if let Err(e) = std::fs::rename(&staging, &destination) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("the extension cannot be put in the store: {e}").into());
    }
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
    // The pending requests the programs made are serviced here too, not dropped.
    projects.poll_programs();
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
            file.and_then(|file| {
                capped(&extension.folder.join(relative(file).ok()?), MAX_LICENCE)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
            })
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
/// was checked this run. The folder is read before anything stops: a manifest that no longer
/// validates, or a folder that is not a surface program's, is refused with its reason while
/// the running program is untouched. A refused or missing extension is refused with its
/// reason.
fn restart(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = Args::new(params).text("id")?;
    // The pending requests the programs made are serviced here too, not dropped.
    projects.poll_programs();
    let folder = projects.extensions.known_mut(id)?.folder.clone();
    let (manifest, value) = Manifest::read(&folder).map_err(CommandError::from)?;
    let Some(runner) = &manifest.runner else {
        return Err(format!("{id:?} is not a surface program; there is nothing to restart").into());
    };
    let Some(script) = manifest.script() else {
        return Err(format!("{id:?}'s manifest names no program file").into());
    };
    // The old program stops before the new one takes its ports.
    let old = projects.extensions.known_mut(id)?.program.take();
    if let Some(old) = old {
        old.stop();
    }
    // The old process's ports are what the new one hears at its handshake: its device is
    // where it was a moment ago, and the router opens the same ports for the new handle at
    // its next look.
    let ports = projects.program_ports(id);
    let handle = match programs::start(id, runner, &folder, script, ports) {
        Ok(handle) => handle,
        Err(reason) => {
            // The old process is already gone: the store says so with the reason, its
            // subscriptions with it, and the router looks again — the ports fell back.
            projects.extensions.start_failed(id, reason.to_string(), manifest, value);
            projects.poll_plugins();
            return Err(reason);
        }
    };
    let extension = projects.extensions.known_mut(id)?;
    extension.manifest = Some(manifest);
    extension.value = Some(value);
    extension.reason = None;
    extension.last_error = None;
    extension.script_stopped = false;
    extension.program = Some(handle);
    extension.state = State::Running;
    // The new program's ports are claimed at once: this look syncs the claims.
    projects.extensions.claims_epoch += 1;
    projects.poll_plugins();
    Ok(json!({ "id": id, "state": "running" }))
}

/// `extensions.remove`: forgets an extension; its program stops first, the user's own folder
/// stays where it is, and the store's copy goes.
fn remove(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = Args::new(params).text("id")?;
    // The pending requests the programs made are serviced here too, not dropped.
    projects.poll_programs();
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
    // The program's ports go back to the usual path at once: this look syncs the claims.
    projects.poll_plugins();
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
    // A rooted path is absolute for the folder's sake on every platform: Windows calls
    // `/etc/passwd` drive-relative, but it never lands in the folder either way.
    if path.is_absolute() || path.has_root() {
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

/// A manifest-named file's size cap: a script this big is far past what a control surface's
/// needs, and the store's scan reads every folder's, so the read is bounded by it.
const MAX_SCRIPT: usize = suhina_plugins::surface::MAX_MESSAGE_BYTES;

/// A licence text's size cap, at validation and at display: the store reads it whole.
const MAX_LICENCE: usize = 256 * 1024;

/// The SHA-256 of a file, hashed as it is read in chunks, no further than `limit`: a file the
/// manifest names is read by the store's scan, so the read is bounded — the error says the
/// file is missing, or it is bigger than the cap.
fn file_digest(file: &Path, limit: usize) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let file = std::fs::File::open(file).map_err(|_| "is missing".to_string())?;
    let mut taken = file.take((limit + 1) as u64);
    let mut hasher = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    let mut total = 0usize;
    loop {
        let read = taken.read(&mut chunk).map_err(|_| "cannot be read".to_string())?;
        if read == 0 {
            break;
        }
        total += read;
        if total > limit {
            return Err(format!("is bigger than {limit} bytes"));
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// A file's bytes, no further than `limit`; one byte past it is refused. The error says the
/// file is missing, or it is bigger than the cap.
fn capped(file: &Path, limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let opened = std::fs::File::open(file).map_err(|_| "is missing".to_string())?;
    let mut bytes = Vec::new();
    opened.take((limit + 1) as u64).read_to_end(&mut bytes).map_err(|_| "cannot be read".to_string())?;
    if bytes.len() > limit {
        return Err(format!("is bigger than {limit} bytes"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod matches {
    use super::port_matches;

    #[test]
    fn a_pattern_matches_whole_names_case_ignored() {
        assert!(port_matches("APC40 mkII", "APC40 mkII"));
        assert!(port_matches("apc40 mkii", "APC40 MKII"));
        assert!(!port_matches("APC40 mkII", "APC40 mkII MIDI 1"));
        assert!(!port_matches("APC40 mkII", "Other"));
    }

    #[test]
    fn a_wildcard_matches_any_run_at_each_place() {
        assert!(port_matches("APC*", "APC40 mkII MIDI 1"));
        assert!(port_matches("*MIDI 1", "APC40 mkII MIDI 1"));
        assert!(port_matches("APC*MIDI 1", "APC40 mkII MIDI 1"));
        assert!(port_matches("*40*", "APC40 mkII MIDI 1"));
        assert!(port_matches("*", "anything at all"));
        // A wildcard matches the empty run too.
        assert!(port_matches("APC*mkII", "APCmkII"));
        assert!(!port_matches("APC*", "Other"));
        assert!(!port_matches("APC* MIDI 1", "APC40 mkII MIDI 2"));
    }

    #[test]
    fn the_pieces_are_found_in_order() {
        assert!(port_matches("AB*C*D", "ABxCD"));
        assert!(port_matches("AB*AB", "ABABAB"));
        // A middle piece found past where the last piece must be does not match.
        assert!(!port_matches("A*M*Z", "AZM"));
    }
}
