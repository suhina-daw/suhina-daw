// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI inputs ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)): which ports
//! there are, which are on, and where the notes they play go. The session only describes the
//! choice; the MIDI layer (`suhina-midi`) lists and opens ports through [`MidiSystem`], which the
//! app or the MCP server hands to [`Projects`]. The notes reach the active project's engine
//! through [`LiveInput`], whichever player that is now. Like the `audio.*` commands, the
//! `midi.*` commands belong to Suhina, not to one project.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};
use suhina_engine::{LiveEvent, LiveSender, LiveSource};

use crate::commands::{CommandKind, CommandSpec, ParamType, param};
use crate::{Agents, CommandError, Policy, Projects};

/// The MIDI input ports of the system, and opening one of them; made by the MIDI layer, which
/// is the only code that opens a MIDI device.
pub trait MidiSystem: Send + Sync {
    /// The input ports there are now, by name.
    fn ports(&self) -> Result<Vec<String>, String>;

    /// Opens the input port `name`; what it plays goes to `input` until the returned value is
    /// dropped, which closes the port.
    fn open(&self, name: &str, input: Arc<LiveInput>) -> Result<Box<dyn std::any::Any + Send>, String>;
}

/// Where the open MIDI ports send what they play: the live-note queue of the active project's
/// engine, or nowhere while no project plays. Called on the MIDI driver's thread (never the
/// audio thread), so it may lock.
#[derive(Debug, Default)]
pub struct LiveInput {
    sender: Mutex<Option<LiveSender>>,
}

impl LiveInput {
    /// Sends `event`, from an unknown port, to the engine that plays now: only tracks that hear
    /// every port play it. Returns false when it went nowhere.
    pub fn send(&self, event: LiveEvent) -> bool {
        self.send_from(LiveSource::default(), event)
    }

    /// Sends `event` from `source` (a port and channel) to the engine that plays now; the armed
    /// tracks whose MIDI input takes the source play it. Returns false when it went nowhere.
    pub fn send_from(&self, source: LiveSource, event: LiveEvent) -> bool {
        self.send_at(source, event, 0)
    }

    /// Sends `event` from `source`, played at `time` on the live clock
    /// ([`suhina_engine::live_now`]; 0 for no time), to the engine that plays now: it sounds a
    /// fixed delay after that time, so notes keep their spacing (#269). Returns false when it
    /// went nowhere.
    pub fn send_at(&self, source: LiveSource, event: LiveEvent, time: u64) -> bool {
        self.lock().as_mut().is_some_and(|sender| sender.send_at(source, event, time))
    }

    /// Notes go to `sender` from now on.
    pub(crate) fn connect(&self, sender: LiveSender) {
        *self.lock() = Some(sender);
    }

    /// Whether an engine takes the notes now.
    pub fn is_connected(&self) -> bool {
        self.lock().as_ref().is_some_and(LiveSender::is_connected)
    }

    fn lock(&self) -> MutexGuard<'_, Option<LiveSender>> {
        // A thread that panicked while sending left nothing half done: the sender is whole.
        self.sender.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The MIDI ports Suhina knows of and which are open: every port is on unless turned off.
#[derive(Default)]
pub(crate) struct MidiInputs {
    pub(crate) system: Option<Arc<dyn MidiSystem>>,
    pub(crate) input: Arc<LiveInput>,
    /// Ports turned off by name (`midi.setInput`, the window's settings).
    pub(crate) off: BTreeSet<String>,
    /// Open ports and what keeps each open.
    open: Vec<(String, Box<dyn std::any::Any + Send>)>,
    /// Ports that could not be opened, with why, so the same failure is said once.
    failed: Vec<(String, String)>,
    /// When the ports were last looked at, so a controller plugged in later is found.
    pub(crate) checked: Option<std::time::Instant>,
    /// The ports there were then, by name.
    pub(crate) ports: Vec<String>,
}

/// How often the ports are looked at again for controllers plugged in or taken out.
pub(crate) const MIDI_RESCAN: std::time::Duration = std::time::Duration::from_secs(2);
/// How often, when the ports could not be listed at all.
const MIDI_RESCAN_FAILED: std::time::Duration = std::time::Duration::from_secs(30);

impl MidiInputs {
    /// Opens the ports that are on and not open yet, and closes the ones turned off or gone.
    /// Returns what the user should hear about: a port that could not be opened, once.
    pub(crate) fn sync(&mut self) -> Vec<String> {
        self.checked = Some(std::time::Instant::now());
        let Some(system) = self.system.clone() else { return Vec::new() };
        let ports = match system.ports() {
            Ok(ports) => ports,
            Err(e) => {
                // A system without MIDI (no sequencer) is asked again far less often.
                let later = std::time::Instant::now().checked_add(MIDI_RESCAN_FAILED.saturating_sub(MIDI_RESCAN));
                self.checked = later.or(self.checked);
                return self.fail("", e);
            }
        };
        self.ports.clone_from(&ports);
        let off = &self.off;
        self.open.retain(|(name, _)| ports.contains(name) && !off.contains(name));
        self.failed.retain(|(name, _)| ports.contains(name) && !off.contains(name));
        let mut notices = Vec::new();
        for name in &ports {
            if self.off.contains(name) || self.open.iter().any(|(open, _)| open == name) {
                continue;
            }
            match system.open(name, Arc::clone(&self.input)) {
                Ok(keep) => {
                    self.failed.retain(|(failed, _)| failed != name);
                    self.open.push((name.clone(), keep));
                }
                Err(e) => notices.extend(self.fail(name, e)),
            }
        }
        notices
    }

    fn fail(&mut self, name: &str, error: String) -> Vec<String> {
        if self.failed.iter().any(|(n, e)| n == name && *e == error) {
            return Vec::new();
        }
        self.failed.retain(|(n, _)| n != name);
        self.failed.push((name.to_string(), error.clone()));
        if name.is_empty() {
            vec![format!("the MIDI inputs could not be listed: {error}")]
        } else {
            vec![format!("the MIDI input {name} could not be opened: {error}")]
        }
    }

    /// Every port there is, with whether it is on, open, and why it could not be opened.
    fn describe(&self) -> Result<Value, CommandError> {
        let system = self.system.as_ref().ok_or_else(no_midi)?;
        let ports = system.ports().map_err(|e| CommandError(format!("the MIDI inputs could not be listed: {e}")))?;
        Ok(Value::Array(
            ports
                .iter()
                .map(|name| {
                    json!({
                        "name": name,
                        "enabled": !self.off.contains(name),
                        "open": self.open.iter().any(|(open, _)| open == name),
                        "error": self.failed.iter().find(|(n, _)| n == name).map(|(_, e)| e),
                    })
                })
                .collect(),
        ))
    }
}

fn no_midi() -> CommandError {
    CommandError("no MIDI inputs here; the desktop app has them, and suhina-cli mcp with --audio".into())
}

static MIDI_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "midi.inputs",
        label: "List the MIDI input ports with whether each is on and open; what the open ones play goes to the armed tracks of the project that plays (track.setArm)",
        params: &[],
        kind: CommandKind::Audio,
        run: inputs,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "midi.setInput",
        label: "Turn a MIDI input port on or off; every port is on until turned off",
        params: &[
            param("port", ParamType::Text, "a port's name from midi.inputs"),
            param("enabled", ParamType::Bool, "true turns it on"),
        ],
        kind: CommandKind::Audio,
        run: set_input,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
];

/// The commands for the MIDI inputs.
pub fn midi_commands() -> &'static [CommandSpec<Projects>] {
    MIDI_COMMANDS
}

fn inputs(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let midi = projects.midi_mut();
    let notices = midi.sync();
    let mut result = json!({ "inputs": midi.describe()?, "connected": midi.input.is_connected() });
    if !notices.is_empty() {
        result["notices"] = json!(notices);
    }
    Ok(result)
}

fn set_input(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let port = params.get("port").and_then(Value::as_str).map(str::trim).ok_or("port must be a text")?;
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let midi = projects.midi_mut();
    let system = midi.system.clone().ok_or_else(no_midi)?;
    let ports = system.ports().map_err(|e| CommandError(format!("the MIDI inputs could not be listed: {e}")))?;
    // A port may be turned off while it is unplugged, so it stays off when it comes back.
    if enabled && !ports.iter().any(|p| p == port) && !midi.off.contains(port) {
        return Err(format!("no MIDI input is called {port:?}; midi.inputs lists them").into());
    }
    if enabled {
        midi.off.remove(port);
    } else {
        midi.off.insert(port.to_string());
    }
    inputs(projects, &Value::Null)
}
