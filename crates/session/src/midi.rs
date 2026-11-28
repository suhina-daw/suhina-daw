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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use serde_json::{Value, json};
use suhina_engine::{LiveEvent, LiveSender, LiveSource};

use crate::commands::Args;
use crate::commands::{CommandKind, CommandSpec, ParamType, optional, param};
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
    /// The input ports there are now, each by name and the system's unique id for it: the id
    /// changes when the device re-enumerates, even when the port was absent too briefly for a
    /// scan to see it gone.
    fn ports(&self) -> Result<Vec<(String, String)>, String>;

    /// Opens the input port `name`; what it plays goes to `input` until the returned value is
    /// dropped, which closes the port.
    fn open(&self, name: &str, input: Arc<LiveInput>) -> Result<Box<dyn std::any::Any + Send>, String>;

    /// Opens with the port's saved expression mode. Backends without MPE keep ordinary MIDI.
    fn open_configured(
        &self,
        name: &str,
        input: Arc<LiveInput>,
        _config: crate::MpeConfig,
    ) -> Result<Box<dyn std::any::Any + Send>, String> {
        self.open(name, input)
    }

    /// The output ports there are now, each by name and the system's unique id for it; a system
    /// with none lists none.
    fn outputs(&self) -> Result<Vec<(String, String)>, String> {
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

/// How many writes one output port's worker holds, and the bytes they may take: the same caps
/// as the programs' send-back queue that feeds it, so one look's whole drain fits and a burst
/// is not lost to a worker thread that has not been scheduled yet. Past them the oldest goes —
/// the newest beats the oldest — and the order of what is sent is kept.
const OUTPUT_QUEUE: usize = 4096;
const OUTPUT_QUEUE_BYTES: usize = 8 * 1024 * 1024;

/// The writes one output port takes, in order: the session's thread enqueues and never waits
/// for the driver — a slow or stalled output held the session's look otherwise (a system
/// exclusive's transmission, a device that is not ready retried without a timeout). A worker
/// of the port's own does the waiting; it ends when the port goes, after the writes it holds.
struct OutputWorker {
    queue: Arc<OutputQueue>,
    /// The all-off reset was queued: a second close after a quit's reset sends nothing twice.
    cleared: AtomicBool,
    /// The program's own quit reset; the safe fallback when it registered none.
    reset: programs::QuitReset,
}

impl OutputWorker {
    fn start(out: Box<dyn MidiOut>, port: String, reset: programs::QuitReset) -> Self {
        let queue = Arc::new(OutputQueue::default());
        let worker = Arc::clone(&queue);
        let _ = std::thread::Builder::new().name("suhina-midi-out".into()).spawn(move || {
            // The writes drain in order; the queue's end (the port was closed or the program
            // stopped) is the worker's end, after the write it is in — nothing joins it, so a
            // wedged driver holds no one.
            while let Some(bytes) = worker.take() {
                if let Err(e) = out.send(&bytes) {
                    suhina_plugins::trace!("midi out {port}: {e}");
                }
                worker.finish();
            }
        });
        Self { queue, cleared: AtomicBool::new(false), reset }
    }

    /// Enqueues the write; a queue at its caps is a driver that cannot keep up, and the oldest
    /// write goes to make room — never a wait on the session's look.
    fn write(&self, bytes: &[u8]) {
        self.queue.push(bytes.to_vec());
    }

    /// Writes the program's quit reset, or a device-safe one when it registered none: notes
    /// off and the channel-mode silences, never CC 0, 7 or 10 (bank select, volume, pan), which
    /// are not the host's to change on a device it does not know. A program that owns its
    /// device's lights registers the whole reset (`suhina.quit_reset`, surface-programs.md).
    fn all_off(&self) {
        if self.cleared.swap(true, Ordering::Relaxed) {
            return;
        }
        let registered = self.reset.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(messages) = registered {
            for bytes in messages {
                self.write(&bytes);
            }
            return;
        }
        for channel in 0..16u8 {
            for note in 0..128u8 {
                self.write(&[0x80 | channel, note, 0]);
            }
            self.write(&[0xB0 | channel, 123, 0]);
            self.write(&[0xB0 | channel, 120, 0]);
            self.write(&[0xB0 | channel, 121, 0]);
        }
    }

    /// Waits until the queue and the write in the driver are done, or the timeout passes: the
    /// quit path waits on this so the reset reaches the device before the process ends.
    fn wait_empty(&self, timeout: std::time::Duration) -> bool {
        self.queue.wait_empty(timeout)
    }
}

impl Drop for OutputWorker {
    fn drop(&mut self) {
        self.queue.close();
    }
}

/// The writes waiting for one output port, with the count and byte caps.
#[derive(Default)]
struct OutputQueue {
    state: Mutex<OutputQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct OutputQueueState {
    writes: std::collections::VecDeque<Vec<u8>>,
    bytes: usize,
    /// One write the worker has taken and not finished sending.
    in_flight: usize,
    closed: bool,
}

impl OutputQueue {
    /// Queues one write, making room by dropping the oldest when the caps are reached.
    fn push(&self, bytes: Vec<u8>) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.writes.len() >= OUTPUT_QUEUE || state.bytes.saturating_add(bytes.len()) > OUTPUT_QUEUE_BYTES {
            let Some(oldest) = state.writes.pop_front() else { break };
            state.bytes = state.bytes.saturating_sub(oldest.len());
        }
        state.bytes = state.bytes.saturating_add(bytes.len());
        state.writes.push_back(bytes);
        self.ready.notify_one();
    }

    /// The oldest write, waiting for one when the queue is empty; `None` when the queue is
    /// closed and drained.
    fn take(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(bytes) = state.writes.pop_front() {
                state.bytes = state.bytes.saturating_sub(bytes.len());
                state.in_flight += 1;
                return Some(bytes);
            }
            if state.closed {
                return None;
            }
            state = self.ready.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// The write the worker took was sent (or failed): the quit path may now finish.
    fn finish(&self) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.in_flight = state.in_flight.saturating_sub(1);
        drop(state);
        self.ready.notify_all();
    }

    /// Whether every write has left the queue and the driver, within `timeout`.
    fn wait_empty(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if state.writes.is_empty() && state.in_flight == 0 {
                return true;
            }
            if state.closed {
                return false;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            let (guard, _) = self.ready.wait_timeout(state, left).unwrap_or_else(std::sync::PoisonError::into_inner);
            state = guard;
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        drop(state);
        self.ready.notify_all();
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
        // An input-sized message past the protocol's cap is not the runner's to receive — the
        // decoder would refuse it after the copy — so it is refused before one.
        if bytes.len() > suhina_plugins::surface::MAX_MESSAGE_BYTES {
            suhina_plugins::trace!("midi in {}: {} bytes, past the cap", self.port, bytes.len());
            return;
        }
        // What the controller plays into the program, next to the session's "midi out": the
        // pair answers why a ring or a pad shows what it shows (docs/debugging.md).
        suhina_plugins::trace!("midi in {port}: {bytes:02x?}", port = self.port);
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

    /// Disconnects the active project without discarding its sending endpoint.
    pub(crate) fn disconnect(&self) -> Option<LiveSender> {
        self.lock().take()
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
    pub(crate) mpe: BTreeMap<String, crate::MpeConfig>,
    /// Open ports and what keeps each open.
    open: Vec<(String, Box<dyn std::any::Any + Send>)>,
    /// Input ports opened raw for a running program: (port, extension id, handle token, keep).
    raw: Vec<(String, String, u64, Box<dyn std::any::Any + Send>)>,
    /// Output ports opened for a running program: (port, extension id, handle token, out).
    outs: Vec<(String, String, u64, OutputWorker)>,
    /// The port sets each program was last told of, so `set_ports` is sent only when they
    /// change or a restart's handle hears them again.
    sent: SentPorts,
    /// Ports that could not be opened, with why, so the same failure is said once.
    failed: Vec<(String, String)>,
    /// When the ports were last looked at, so a controller plugged in later is found.
    pub(crate) checked: Option<std::time::Instant>,
    /// The ports there were then, by name.
    pub(crate) ports: Vec<String>,
    /// The system's unique id for each port there was then: a port that shows up again with a
    /// different id re-enumerated, even when it was gone too briefly for a scan to see.
    ids: std::collections::BTreeMap<String, String>,
    /// The claims epoch the last look synced at, so a changed claim set is seen at once
    /// ([`crate::extensions::Extensions::claims_epoch`]).
    pub(crate) synced_epoch: u64,
    /// The output ports there were at the last look, so a port that vanished and came back is
    /// told from one that stayed.
    last_outputs: Vec<String>,
    /// The system's unique id for each output port there was then, as [`Self::ids`] for inputs.
    out_ids: std::collections::BTreeMap<String, String>,
}

/// How often the ports are looked at again for controllers plugged in or taken out.
pub(crate) const MIDI_RESCAN: std::time::Duration = std::time::Duration::from_secs(2);
/// How often, when the ports could not be listed at all.
const MIDI_RESCAN_FAILED: std::time::Duration = std::time::Duration::from_secs(30);

/// The system's ports for one look, as the scan took them: the input ports there are now,
/// with the system's id for each, then the output ports the same way. The names are what a
/// program's manifest patterns are matched against, and what the store starts a program on.
pub(crate) struct Scanned {
    pub(crate) inputs: Vec<(String, String)>,
    pub(crate) outputs: Vec<(String, String)>,
}

impl MidiInputs {
    /// The ports a program holds now, by extension id: the raw inputs opened for it and the
    /// outputs opened for its send-backs. What a restart's new process hears at its
    /// handshake, before the router's next look opens the same ports for it again.
    pub(crate) fn held(&self, id: &str) -> (Vec<String>, Vec<String>) {
        (
            self.raw.iter().filter(|(_, held_id, _, _)| held_id == id).map(|(name, ..)| name.clone()).collect(),
            self.outs.iter().filter(|(_, held_id, _, _)| held_id == id).map(|(name, ..)| name.clone()).collect(),
        )
    }

    /// The system's ports for this look: the input ports there are now, with the system's id
    /// for each, then the output ports the same way. `Err` is why the ports could not be
    /// listed, said once — a system without MIDI (no sequencer) is asked again far less
    /// often.
    pub(crate) fn scan(&mut self) -> Result<Scanned, Vec<String>> {
        self.checked = Some(std::time::Instant::now());
        let Some(system) = self.system.clone() else {
            return Ok(Scanned { inputs: Vec::new(), outputs: Vec::new() });
        };
        let ports = match system.ports() {
            Ok(ports) => ports,
            Err(e) => {
                let later = std::time::Instant::now().checked_add(MIDI_RESCAN_FAILED.saturating_sub(MIDI_RESCAN));
                self.checked = later.or(self.checked);
                return Err(self.fail("", e));
            }
        };
        let outputs = system.outputs().unwrap_or_default();
        Ok(Scanned { inputs: ports, outputs })
    }

    /// Opens the ports that are on and not open yet, and closes the ones turned off or gone;
    /// opens the ports each running program claims raw, and its output ports, and tells each
    /// program its ports when they change. A port a program claims does not feed the armed
    /// tracks while it runs. `scanned` is what [`Self::scan`] took this look: the names the
    /// claims were matched against and the programs started on. Returns what the user should
    /// hear about: a port that could not be opened, once.
    pub(crate) fn sync(&mut self, claims: &[Claim], epoch: u64, scanned: Scanned) -> Vec<String> {
        self.synced_epoch = epoch;
        let Some(system) = self.system.clone() else { return Vec::new() };
        let Scanned { inputs: ports, outputs } = scanned;
        // A port that vanished and came back between looks re-enumerated: its old connection
        // died with the device, so every entry it held is dropped here and reopened fresh
        // below — the claim by token would otherwise keep a connection to nothing. A port that
        // never left the list but answers with a new unique id re-enumerated inside one scan —
        // the same death, invisible to a look that only reads names, so the id tells it.
        let port_names: Vec<String> = ports.iter().map(|(n, _)| n.clone()).collect();
        let out_names: Vec<String> = outputs.iter().map(|(n, _)| n.clone()).collect();
        let returned: BTreeSet<String> =
            ports.iter().filter(|(p, _)| !self.ports.contains(p)).map(|(p, _)| p.clone()).collect();
        let returned_outs: BTreeSet<String> =
            outputs.iter().filter(|(p, _)| !self.last_outputs.contains(p)).map(|(p, _)| p.clone()).collect();
        let renumbered: BTreeSet<String> = ports
            .iter()
            .filter(|(p, id)| self.ids.get(p).is_some_and(|held| held != id))
            .map(|(p, _)| p.clone())
            .collect();
        let renumbered_outs: BTreeSet<String> = outputs
            .iter()
            .filter(|(p, id)| self.out_ids.get(p).is_some_and(|held| held != id))
            .map(|(p, _)| p.clone())
            .collect();
        self.ports = port_names.clone();
        self.last_outputs = out_names.clone();
        self.ids = ports.iter().cloned().collect();
        self.out_ids = outputs.iter().cloned().collect();
        // What each running program claims: the ports its patterns match, the first program
        // whose patterns match a port taking it (two manifests may both name one port).
        let mut taken: BTreeSet<String> = BTreeSet::new();
        let mut matched: Vec<(&str, Vec<String>, Vec<String>)> = Vec::new();
        for claim in claims {
            let free =
                |name: &&String| !taken.contains(*name) && claim.patterns.iter().any(|pat| port_matches(pat, name));
            let inputs: Vec<String> = port_names.iter().filter(free).cloned().collect();
            let outs: Vec<String> = out_names.iter().filter(free).cloned().collect();
            taken.extend(inputs.iter().cloned());
            taken.extend(outs.iter().cloned());
            matched.push((claim.id, inputs, outs));
        }
        let mut notices = Vec::new();
        // A returned or re-enumerated port's stale entries go first: its old connection died
        // with the device either way.
        let stale: BTreeSet<String> = returned.union(&renumbered).cloned().collect();
        let stale_outs: BTreeSet<String> = returned_outs.union(&renumbered_outs).cloned().collect();
        self.raw.retain(|(name, ..)| !stale.contains(name));
        self.outs.retain(|(name, ..)| !stale_outs.contains(name) && !stale.contains(name));
        self.open.retain(|(name, _)| !stale.contains(name));
        // Programs that stopped: their ports close first, so a port another program claims
        // now can be opened for it.
        self.raw.retain(|(_, id, _, _)| matched.iter().any(|(id2, _, _)| id2 == id));
        self.outs.retain(|(_, id, _, _)| matched.iter().any(|(id2, _, _)| id2 == id));
        // A claimed port's ordinary connection goes before the claim's raw open: a driver
        // that takes one connection to a port refuses the second, and the displaced
        // connection's close must not come after the open it displaced.
        self.open.retain(|(name, _)| !taken.contains(name));
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
                    Ok(out) => self.outs.push((
                        name.clone(),
                        id.to_string(),
                        token,
                        OutputWorker::start(out, name.clone(), claim.handle.quit_reset()),
                    )),
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
        self.open.retain(|(name, _)| port_names.contains(name) && !off.contains(name) && !claimed.contains(name));
        self.failed.retain(|(name, _)| port_names.contains(name) && !off.contains(name));
        for name in &port_names {
            if self.off.contains(name) || claimed.contains(name) || self.open.iter().any(|(open, _)| open == name) {
                continue;
            }
            match system.open_configured(name, Arc::clone(&self.input), self.mpe.get(name).copied().unwrap_or_default())
            {
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
        // What the program lights, next to the driver's "midi in": the pair answers why a ring
        // or a pad shows what it shows (docs/debugging.md). The write is the worker's: the
        // session's look is not held by the driver.
        suhina_plugins::trace!("midi out {port}: {bytes:02x?}");
        out.write(bytes);
    }

    /// Sends each program's quit reset on its outputs, then waits until `deadline` for the
    /// workers to write them: quitting must leave a controller dark, not lit by its last frame.
    /// The same deadline bounds the close that follows, so quit is not given two full waits.
    pub(crate) fn reset_outputs(&self, deadline: std::time::Instant) {
        // The last frame's writes go first, so the reset is not pushed out by them; then the
        // offs, then their writes, before the app closes the ports.
        for (_, _, _, out) in &self.outs {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() || !out.wait_empty(left) {
                break;
            }
        }
        for (_, _, _, out) in &self.outs {
            out.all_off();
        }
        for (_, _, _, out) in &self.outs {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() || !out.wait_empty(left) {
                break;
            }
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
    /// running program), and why it could not be opened. The system's unique id rides along:
    /// it changes when the device re-enumerates, which is how a port that blinked off the bus
    /// too briefly for a scan is told after the fact.
    fn describe(&self) -> Result<Value, CommandError> {
        let system = self.system.as_ref().ok_or_else(no_midi)?;
        let ports =
            system.ports().map_err(|e| CommandError::new(format!("the MIDI inputs could not be listed: {e}")))?;
        Ok(Value::Array(
            ports
                .iter()
                .map(|(name, id)| {
                    let claimed = self.raw.iter().any(|(p, _, _, _)| p == name);
                    json!({
                        "name": name,
                        "id": id,
                        "enabled": !self.off.contains(name),
                        "open": self.open.iter().any(|(open, _)| open == name) || claimed,
                        "claimed": claimed,
                        "error": self.failed.iter().find(|(n, _)| n == name).map(|(_, e)| e),
                        "mpe": self.mpe.get(name).copied().unwrap_or_default().to_json(),
                    })
                })
                .collect(),
        ))
    }
}

fn no_midi() -> CommandError {
    CommandError::new("no MIDI inputs here; the desktop app has them, and suhina-cli mcp with --audio".into())
}

static MIDI_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "midi.setMpe",
        label: "Configure per-note expression for one MIDI input; changes choke its old notes before reopening it",
        params: &[
            param("port", ParamType::Text, "physical input name from midi.inputs"),
            param("mode", ParamType::Text, "off, automatic, or manual"),
            optional("lowerMembers", ParamType::Integer, "lower-zone members, 0 to 15, excluding channel 1"),
            optional("upperMembers", ParamType::Integer, "upper-zone members, 0 to 15, excluding channel 16"),
            optional("lowerMemberBend", ParamType::Integer, "member pitch range, 0 to 96 semitones"),
            optional("lowerManagerBend", ParamType::Integer, "manager pitch range, 0 to 96 semitones"),
            optional("upperMemberBend", ParamType::Integer, "member pitch range, 0 to 96 semitones"),
            optional("upperManagerBend", ParamType::Integer, "manager pitch range, 0 to 96 semitones"),
        ],
        kind: CommandKind::on(crate::commands::Target::Audio, crate::commands::Effect::Change),
        run: set_mpe,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
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
    let ports = system.ports().map_err(|e| CommandError::new(format!("the MIDI inputs could not be listed: {e}")))?;
    // A port may be turned off while it is unplugged, so it stays off when it comes back.
    if enabled && !ports.iter().any(|(p, _)| p == port) && !midi.off.contains(port) {
        return Err(format!("no MIDI input is called {port:?}; midi.inputs lists them").into());
    }
    if enabled {
        midi.off.remove(port);
    } else {
        midi.off.insert(port.to_string());
    }
    inputs(projects, &Value::Null)
}

fn set_mpe(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    // The name exactly as `midi.inputs` lists it: a system's port name may begin or end with
    // spaces, and a trimmed name would not find that port.
    let port = args.raw_str("port").ok_or("port must be a text")?;
    if port.trim().is_empty() || port.len() > 1024 {
        return Err("port must contain 1 to 1024 bytes".into());
    }
    let midi = projects.midi_mut();
    let mut config = midi.mpe.get(port).copied().unwrap_or_default();
    config.mode = match args.text("mode")? {
        "off" => crate::MpeMode::Off,
        "automatic" => crate::MpeMode::Automatic,
        "manual" => crate::MpeMode::Manual,
        _ => return Err("mode must be off, automatic, or manual".into()),
    };
    for (name, field) in [
        ("lowerMembers", &mut config.lower_members),
        ("upperMembers", &mut config.upper_members),
        ("lowerMemberBend", &mut config.lower_member_bend),
        ("lowerManagerBend", &mut config.lower_manager_bend),
        ("upperMemberBend", &mut config.upper_member_bend),
        ("upperManagerBend", &mut config.upper_manager_bend),
    ] {
        if args.has(name) {
            *field = u8::try_from(args.index(name, "from zero")?)
                .map_err(|_| CommandError::from("zone values must fit 0 to 96"))?;
        }
    }
    if !config.is_valid() {
        return Err("MPE zones must not overlap: each count is 0 to 15, two active zones have at most 14 members total; bend ranges are 0 to 96 semitones".into());
    }
    let system = midi.system.clone().ok_or_else(no_midi)?;
    if !system.ports().map_err(CommandError::from)?.iter().any(|(name, _)| name == port) && !midi.mpe.contains_key(port)
    {
        return Err(format!("no MIDI input is called {port:?}; midi.inputs lists them").into());
    }
    if !midi.mpe.contains_key(port) && midi.mpe.len() >= 256 {
        return Err("at most 256 MIDI input configurations are retained".into());
    }
    if midi.mpe.get(port).copied().unwrap_or_default() != config {
        midi.mpe.insert(port.to_owned(), config);
        midi.open.retain(|(name, _)| name != port);
        midi.failed.retain(|(name, _)| name != port);
    }
    // The configuration is in place from here on: a listing that fails now is a notice, not
    // a failure, so the caller keeps (and saves) what the command did.
    let port = port.to_owned();
    inputs(projects, &Value::Null).or_else(|error| {
        Ok(json!({
            "port": port,
            "mpe": config.to_json(),
            "notices": [format!("the MIDI inputs could not be listed: {}", error.message())],
        }))
    })
}

#[cfg(test)]
#[path = "midi_tests.rs"]
mod tests;
