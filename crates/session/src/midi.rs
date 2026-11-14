// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI inputs and outputs ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)):
//! which ports there are, which are on, and where the notes they play go. The session only
//! describes the choice; the MIDI layer (`suhina-midi`) lists and opens ports through
//! [`MidiSystem`], which the app or the MCP server hands to [`Projects`]. The notes reach the
//! active project's engine through [`LiveInput`], whichever player that is now. Like the
//! `audio.*` commands, the `midi.*` commands belong to Suhina, not to one project.
//!
//! A port a running surface program claims ([ADR 0019](../../../docs/decisions/0019-midi-controllers-and-mapping.md)
//! › 10) is opened raw instead: every message whole, system exclusive included, goes to the
//! program through a [`RawSink`], and what the program sends back is written to its output
//! ports through [`MidiOut`]. A claimed port does not feed the armed tracks while its program
//! runs; when the program stops, the port goes back to the usual path.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};
use suhina_engine::{LiveEvent, LiveSender, LiveSource};

use crate::commands::Args;
use crate::commands::{CommandKind, CommandSpec, ParamType, param};
use crate::extensions::programs;
use crate::extensions::{Claim, port_matches};
use crate::{Agents, CommandError, Policy, Projects};

/// Writes bytes to one open MIDI output port; made by the MIDI layer, which is the only code
/// that opens a MIDI device.
pub trait MidiOut: Send {
    /// Writes the whole message; an error says the port is gone.
    fn send(&self, bytes: &[u8]) -> Result<(), String>;
}

/// Where a claimed input port's raw bytes go: the running program, through the router's sink.
/// Called on the MIDI driver's thread (never the audio thread).
pub trait RawSink: Send + Sync {
    /// The whole message as the driver read it, system exclusive included, with its timestamp
    /// in microseconds.
    fn message(&self, stamp: u64, bytes: &[u8]);
}

/// The MIDI input ports of the system, and opening one of them; made by the MIDI layer, which
/// is the only code that opens a MIDI device.
pub trait MidiSystem: Send + Sync {
    /// The input ports there are now, by name.
    fn ports(&self) -> Result<Vec<String>, String>;

    /// Opens the input port `name`; what it plays goes to `input` until the returned value is
    /// dropped, which closes the port.
    fn open(&self, name: &str, input: Arc<LiveInput>) -> Result<Box<dyn std::any::Any + Send>, String>;

    /// The output ports there are now, by name; a system with none lists none.
    fn outputs(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    /// Opens the output port `name` for writing; the returned value keeps it open, and
    /// dropping it closes the port.
    fn open_output(&self, name: &str) -> Result<Box<dyn MidiOut>, String> {
        let _ = name;
        Err("this MIDI system opens no output ports".into())
    }

    /// Opens the input port `name` raw: every message whole, system exclusive included, goes
    /// to `sink` with the driver's timestamp, until the returned value is dropped, which
    /// closes the port. Nothing is parsed and none of it reaches the live notes.
    fn open_raw(&self, name: &str, sink: Arc<dyn RawSink>) -> Result<Box<dyn std::any::Any + Send>, String> {
        let _ = (name, sink);
        Err("this MIDI system opens no raw input ports".into())
    }
}

/// Delivers a claimed port's raw messages to its running program: the router's sink, one per
/// opened port. The driver's thread calls it, and it only sends on the program's order
/// channel, which the supervisor's loop drains.
struct ProgramSink {
    to: std::sync::mpsc::Sender<programs::ToProgram>,
    port: String,
}

impl RawSink for ProgramSink {
    fn message(&self, stamp: u64, bytes: &[u8]) {
        let _ = self.to.send(programs::ToProgram::PortMessage {
            port: self.port.clone(),
            timestamp: stamp,
            bytes: bytes.to_vec(),
        });
    }
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

/// The port sets each program was last told of, keyed by (extension id, handle token).
type SentPorts = Vec<(String, u64, (Vec<String>, Vec<String>))>;

/// The MIDI ports Suhina knows of and which are open: every port is on unless turned off.
#[derive(Default)]
pub(crate) struct MidiInputs {
    pub(crate) system: Option<Arc<dyn MidiSystem>>,
    pub(crate) input: Arc<LiveInput>,
    /// Ports turned off by name (`midi.setInput`, the window's settings).
    pub(crate) off: BTreeSet<String>,
    /// Open ports and what keeps each open.
    open: Vec<(String, Box<dyn std::any::Any + Send>)>,
    /// Input ports opened raw for a running program: (port, extension id, handle token, keep).
    raw: Vec<(String, String, u64, Box<dyn std::any::Any + Send>)>,
    /// Output ports opened for a running program: (port, extension id, handle token, out).
    outs: Vec<(String, String, u64, Box<dyn MidiOut>)>,
    /// The port sets each program was last told of, so `set_ports` is sent only when they
    /// change or a restart's handle hears them again.
    sent: SentPorts,
    /// Ports that could not be opened, with why, so the same failure is said once.
    failed: Vec<(String, String)>,
    /// When the ports were last looked at, so a controller plugged in later is found.
    pub(crate) checked: Option<std::time::Instant>,
    /// The ports there were then, by name.
    pub(crate) ports: Vec<String>,
    /// The claims epoch the last look synced at, so a changed claim set is seen at once
    /// ([`crate::extensions::Extensions::claims_epoch`]).
    pub(crate) synced_epoch: u64,
}

/// How often the ports are looked at again for controllers plugged in or taken out.
pub(crate) const MIDI_RESCAN: std::time::Duration = std::time::Duration::from_secs(2);
/// How often, when the ports could not be listed at all.
const MIDI_RESCAN_FAILED: std::time::Duration = std::time::Duration::from_secs(30);

impl MidiInputs {
    /// Opens the ports that are on and not open yet, and closes the ones turned off or gone;
    /// opens the ports each running program claims raw, and its output ports, and tells each
    /// program its ports when they change. A port a program claims does not feed the armed
    /// tracks while it runs. Returns what the user should hear about: a port that could not be
    /// opened, once.
    pub(crate) fn sync(&mut self, claims: &[Claim], epoch: u64) -> Vec<String> {
        self.checked = Some(std::time::Instant::now());
        self.synced_epoch = epoch;
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
        let outputs = system.outputs().unwrap_or_default();
        // What each running program claims: the ports its patterns match, the first program
        // whose patterns match a port taking it (two manifests may both name one port).
        let mut taken: BTreeSet<String> = BTreeSet::new();
        let mut matched: Vec<(&str, Vec<String>, Vec<String>)> = Vec::new();
        for claim in claims {
            let free =
                |name: &&String| !taken.contains(*name) && claim.patterns.iter().any(|pat| port_matches(pat, name));
            let inputs: Vec<String> = ports.iter().filter(free).cloned().collect();
            let outs: Vec<String> = outputs.iter().filter(free).cloned().collect();
            taken.extend(inputs.iter().cloned());
            taken.extend(outs.iter().cloned());
            matched.push((claim.id, inputs, outs));
        }
        let mut notices = Vec::new();
        // Programs that stopped: their ports close first, so a port another program claims
        // now can be opened for it.
        self.raw.retain(|(_, id, _, _)| matched.iter().any(|(id2, _, _)| id2 == id));
        self.outs.retain(|(_, id, _, _)| matched.iter().any(|(id2, _, _)| id2 == id));
        // The claim pass: open what a running program claims and does not hold yet, and drop
        // what it no longer claims or what a restart's new handle must hear again.
        for (id, inputs, outs) in &matched {
            let Some(claim) = claims.iter().find(|claim| claim.id == *id) else { continue };
            let token = claim.handle.token();
            self.raw.retain(|(name, held_id, _, _)| !(held_id == *id && !inputs.contains(name)));
            self.outs.retain(|(name, held_id, _, _)| !(held_id == *id && !outs.contains(name)));
            for name in inputs {
                if self.raw.iter().any(|(p, held_id, held_token, _)| p == name && held_id == id && *held_token == token)
                {
                    continue;
                }
                // A restart's new handle: the entry keyed by the old token goes, and the
                // program hears its whole port set again through the new one.
                self.raw.retain(|(p, held_id, _, _)| !(p == name && held_id == id));
                let sink: Arc<dyn RawSink> = Arc::new(ProgramSink { to: claim.handle.sender(), port: name.clone() });
                match system.open_raw(name, sink) {
                    Ok(keep) => self.raw.push((name.clone(), id.to_string(), token, keep)),
                    Err(e) => notices.extend(self.fail(name, e)),
                }
            }
            for name in outs {
                if self
                    .outs
                    .iter()
                    .any(|(p, held_id, held_token, _)| p == name && held_id == id && *held_token == token)
                {
                    continue;
                }
                self.outs.retain(|(p, held_id, _, _)| !(p == name && held_id == id));
                match system.open_output(name) {
                    Ok(out) => self.outs.push((name.clone(), id.to_string(), token, out)),
                    Err(e) => notices.extend(self.fail(name, e)),
                }
            }
            // The ports the program holds now, sent whole when they change.
            let held_inputs: Vec<String> = inputs
                .iter()
                .filter(|p| self.raw.iter().any(|(n, i, t, _)| n == *p && i == id && *t == token))
                .cloned()
                .collect();
            let held_outs: Vec<String> = outs
                .iter()
                .filter(|p| self.outs.iter().any(|(n, i, t, _)| n == *p && i == id && *t == token))
                .cloned()
                .collect();
            let known = (held_inputs.clone(), held_outs.clone());
            if !self.sent.iter().any(|(i, t, sets)| i == id && *t == token && *sets == known) {
                claim.handle.set_ports(held_inputs, held_outs);
                self.sent.retain(|(i, _, _)| i != id);
                self.sent.push((id.to_string(), token, known));
            }
        }
        self.sent.retain(|(id, _, _)| matched.iter().any(|(id2, _, _)| id2 == id));
        // The usual pass: every port that is on, unclaimed, and not open yet.
        let off = &self.off;
        let claimed = &taken;
        self.open.retain(|(name, _)| ports.contains(name) && !off.contains(name) && !claimed.contains(name));
        self.failed.retain(|(name, _)| ports.contains(name) && !off.contains(name));
        for name in &ports {
            if self.off.contains(name) || claimed.contains(name) || self.open.iter().any(|(open, _)| open == name) {
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

    /// Writes a program's send-back to the output port held for it. A port the program holds
    /// no output for drops the bytes: the manifest claimed no such output, or it is not open.
    pub(crate) fn send_back(&mut self, program: &str, port: &str, bytes: &[u8]) {
        let Some((_, _, _, out)) = self.outs.iter().find(|(p, id, _, _)| p == port && id == program) else {
            return;
        };
        if let Err(e) = out.send(bytes) {
            suhina_plugins::trace!("midi out {port}: {e}");
        }
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

    /// Every port there is, with whether it is on, open (by the session, or claimed raw by a
    /// running program), and why it could not be opened.
    fn describe(&self) -> Result<Value, CommandError> {
        let system = self.system.as_ref().ok_or_else(no_midi)?;
        let ports = system.ports().map_err(|e| CommandError(format!("the MIDI inputs could not be listed: {e}")))?;
        Ok(Value::Array(
            ports
                .iter()
                .map(|name| {
                    let claimed = self.raw.iter().any(|(p, _, _, _)| p == name);
                    json!({
                        "name": name,
                        "enabled": !self.off.contains(name),
                        "open": self.open.iter().any(|(open, _)| open == name) || claimed,
                        "claimed": claimed,
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
        kind: CommandKind::on(crate::commands::Target::Audio, crate::commands::Effect::Read),
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
        kind: CommandKind::on(crate::commands::Target::Audio, crate::commands::Effect::Change),
        run: set_input,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
];

/// The commands for the MIDI inputs.
pub fn midi_commands() -> &'static [CommandSpec<Projects>] {
    MIDI_COMMANDS
}

fn inputs(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let notices = projects.midi_sync();
    let midi = projects.midi_mut();
    let mut result = json!({ "inputs": midi.describe()?, "connected": midi.input.is_connected() });
    if !notices.is_empty() {
        result["notices"] = json!(notices);
    }
    Ok(result)
}

fn set_input(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let port = Args::new(params).raw_str("port").map(str::trim).ok_or("port must be a text")?;
    let enabled = Args::new(params).bool("enabled")?;
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
