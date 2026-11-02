// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine's side of plug-in hosting (ADR 0006): starting `suhina-plugin-host` processes,
//! loading plug-in instances into them, talking to them, noticing when one crashes or hangs, and
//! restarting it with its last state.
//!
//! [`Plugin::start`] returns two halves. [`PluginAudio`] goes to the audio thread and runs
//! blocks through the plug-in with a deadline ([`suhina_ipc`]); [`Plugin`] stays on a control
//! thread for everything else: health, ping, state, restart. They share only a counter of
//! blocks in a row the host has not answered.
//!
//! One host process can run several instances ([`Launch::group`]): instances started with the
//! same host executable and the same group share the process that a registry in this crate
//! keeps for the group, each with its own audio link. Their requests to the process are taken
//! in turn. When a restart replaces the group's process, the group's generation goes up; the
//! instances loaded into the earlier process then report [`Health::Crashed`], and their own
//! restart loads them into the new process.
//!
//! In detail: The engine's side of plug-in hosting: `Plugin::start` loads an instance into a
//! `suhina-plugin-host` process (the one its group shares, `Launch::group`, or one of its own) and
//! returns a control half (health, ping, state, parameters, restart) and an audio half
//! (`PluginAudio::process`, real-time safe). Control messages are JSON lines over a local socket
//! (`protocol`; the host also reports its process's resident memory once a second, unasked, and
//! `host_memory` reads the running hosts' reports for the control bar). A host that exits is
//! *crashed*; one that leaves 32 blocks in a row unanswered is *hung*; `restart` brings it back
//! with its last saved state. `scan` and `scan_pool` find installed CLAP and VST3 plug-ins
//! (standard folders, `CLAP_PATH`, folders the user adds), read VST3 bundles' `moduleinfo.json`
//! without loading them and load the rest in a few long-lived `--scan-worker` processes with
//! per-file timeouts, into a catalog that can be saved. `PluginAudio::process_with_events` sends an
//! instrument its notes. `offline::render` runs audio (and `render_with_events` notes) through a
//! plug-in without a deadline (`suhina-cli plugin run`, `plugin run-device`). The session starts a
//! project's plug-ins with it and hands their audio halves to the engine.

pub mod arch;
pub mod library;
pub mod matching;
pub mod offline;
pub mod protocol;
pub mod scan;
pub mod scan_pool;
pub mod trace;
pub mod vst2;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use interprocess::TryClone;
use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Name, NameType, Stream, ToFsName,
    ToNsName,
};
pub use suhina_ipc::{
    AudioSource, Event, EventKind, MAX_EVENTS, Outcome, keys_to_engine, pointer_on_screen, set_keys_to_engine,
    take_keyboard_back,
};
use suhina_ipc::{IpcError, Link};

use protocol::{AraDocument, MAX_LINE, Message, VERSION};
pub use protocol::{ParamInfo, Placement};

/// Blocks in a row without an answer after which a running host counts as hung.
pub const HUNG_AFTER: u32 = 32;

/// The longest an instance's unload is waited for (or the launch timeout, when shorter): a host
/// stuck in a plug-in's teardown then ends instead of holding up the window that let it go.
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Editor closings (and sizes, and latencies) remembered for instances that have not asked yet.
const MAX_CLOSED_EDITORS: usize = 4096;

#[derive(Debug)]
pub enum PluginError {
    Io(std::io::Error),
    Ipc(IpcError),
    /// The host did not answer in time; the text says to what.
    Timeout(&'static str),
    /// The host broke the protocol or went away.
    Protocol(String),
    /// The host reported an error, such as a plug-in it cannot load.
    Host(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "plug-in host: {e}"),
            Self::Ipc(e) => write!(f, "plug-in host: {e}"),
            Self::Timeout(what) => write!(f, "plug-in host did not answer: {what}"),
            Self::Protocol(why) => write!(f, "plug-in host: {why}"),
            Self::Host(message) => write!(f, "plug-in host: {message}"),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<IpcError> for PluginError {
    fn from(e: IpcError) -> Self {
        Self::Ipc(e)
    }
}

/// How to start a plug-in.
#[derive(Debug, Clone)]
pub struct Launch {
    /// The `suhina-plugin-host` executable.
    pub host: PathBuf,
    /// What to load, as the host names it.
    pub plugin: String,
    pub channels: u32,
    pub max_frames: u32,
    pub sample_rate: f64,
    /// How long to wait for the host to start, load the plug-in or answer a request.
    pub timeout: Duration,
    /// Instances started with the same `host` and the same group share one host process
    /// (ADR 0006: by default one per plug-in file; see [`library::group_for`]). `None` gives
    /// the instance a process of its own.
    pub group: Option<String>,
    /// The ARA document to bind the plug-in to as it loads, for an ARA plug-in on a track
    /// (ADR 0033); `None` for every other plug-in.
    pub ara: Option<protocol::AraDocument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Running,
    /// The host process has exited, or a restart of another instance in it replaced it.
    Crashed,
    /// The host process runs but has not answered [`HUNG_AFTER`] blocks of this instance in a
    /// row.
    Hung,
}

/// The audio thread's half: runs blocks through the plug-in.
pub struct PluginAudio {
    /// Used only while the renderer is quiescent, never by a render callback.
    group: std::sync::Weak<Mutex<Group>>,
    sample_rate: Arc<AtomicU64>,
    link: Link,
    unanswered: Arc<AtomicU32>,
    /// The instance's number in its host process.
    instance: u32,
    /// The host process it was loaded into (its group's generation then).
    process: u64,
}

/// Where a plug-in instance runs, for a block that several instances of one host process run
/// in one round trip ([`PluginAudio::send_joint`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seat {
    /// The host process: the same for every instance loaded into the same process.
    pub process: u64,
    /// The instance's number in it.
    pub instance: u32,
}

impl PluginAudio {
    /// Reconfigures a quiescent audio link. Not real-time safe: call only after the old
    /// renderer has stopped and before the new one starts. The control half may stay on
    /// its session thread, including while an editor is open.
    pub fn reconfigure(&mut self, sample_rate: f64, timeout: Duration) -> Result<u32, PluginError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || sample_rate > 768_000.0 {
            return Err(PluginError::Protocol("invalid sample rate".into()));
        }
        let shared = self.group.upgrade().ok_or_else(|| PluginError::Protocol("the plug-in was unloaded".into()))?;
        let deadline = Instant::now() + timeout;
        let mut group = lock_until(&shared, deadline).ok_or(PluginError::Timeout("reconfigure"))?;
        if group.generation != self.process {
            return Err(PluginError::Protocol("the host process was replaced".into()));
        }
        let instance = self.instance;
        let reply = group.request(
            Some(instance),
            &Message::Reconfigure { instance, sample_rate },
            "reconfigure",
            deadline.saturating_duration_since(Instant::now()),
            |m| matches!(m, Message::Reconfigured { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::Reconfigured { latency, .. } => {
                self.sample_rate.store(sample_rate.to_bits(), Ordering::Release);
                group.note(&Message::LatencyChanged { instance, latency });
                Ok(latency)
            }
            _ => Err(PluginError::Protocol("expected reconfiguration acknowledgement".into())),
        }
    }

    /// One block through the plug-in, waiting at most `timeout`; see [`Link::process`].
    /// Real-time safe.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], timeout: Duration) -> Outcome {
        self.process_with_events(input, &[], output, timeout)
    }

    /// [`PluginAudio::process`] with notes and controllers at frames of the block, for an
    /// instrument; see [`Link::process_with_events`]. Real-time safe.
    pub fn process_with_events(
        &mut self,
        input: &[f32],
        events: &[Event],
        output: &mut [f32],
        timeout: Duration,
    ) -> Outcome {
        if self.link.ready() {
            self.unanswered.store(0, Ordering::Relaxed);
        }
        let outcome = self.link.process_with_events(input, events, output, timeout);
        self.note(outcome);
        outcome
    }

    /// The first half of [`PluginAudio::process_with_events`]: sends the block (at most
    /// `room` samples, the output's size) without waiting for its answer, so several plug-ins'
    /// blocks can be out at once. When the host is still busy with an earlier block it waits
    /// up to `patience` for that one to be answered (an offline render; in real time
    /// `patience` is zero). `false` when the block could not go: counted as unanswered, once.
    /// Real-time safe.
    pub fn send(&mut self, input: &[f32], events: &[Event], room: usize, patience: Duration) -> bool {
        let mut sent = self.link.send_with_events(input, events, room);
        if !sent && !patience.is_zero() && self.link.wait_ready(Instant::now() + patience) {
            sent = self.link.send_with_events(input, events, room);
        }
        if sent {
            // Accepting a new request proves the previous one was answered, even
            // if its reply arrived too late to be used for audio.
            self.unanswered.store(0, Ordering::Relaxed);
        } else {
            self.note(Outcome::Busy);
        }
        sent
    }

    /// Where the instance runs, while it can take part in a joint block: its link has no block
    /// out and every block sent has been answered (one that has not would make the others in the
    /// joint block wait for it).
    pub fn seat(&self) -> Option<Seat> {
        self.link.ready().then_some(Seat { process: self.process, instance: self.instance })
    }

    /// [`PluginAudio::send`] for a block that the instances `followers` of the same host process
    /// run after this one, in order, with the same events, before the host answers
    /// ([`Link::send_joint`]); [`PluginAudio::finish`] takes the last one's output. Their own
    /// links stay idle; tell each how the block went with [`PluginAudio::followed`].
    /// Real-time safe.
    pub fn send_joint(
        &mut self,
        input: &[f32],
        events: &[Event],
        followers: &[u32],
        room: usize,
        patience: Duration,
    ) -> bool {
        let mut sent = self.link.send_joint(input, events, followers, room);
        if !sent && !patience.is_zero() && self.link.wait_ready(Instant::now() + patience) {
            sent = self.link.send_joint(input, events, followers, room);
        }
        if sent {
            self.unanswered.store(0, Ordering::Relaxed);
        } else {
            self.note(Outcome::Busy);
        }
        sent
    }

    /// Counts a joint block this instance followed in, as another instance's link said it went:
    /// a block its host did not answer counts against this instance too, as it may be the one
    /// that is stuck. Real-time safe.
    pub fn followed(&self, outcome: Outcome) {
        // A missed joint request was accepted, so the previous joint request
        // finished. Busy means no new request could go and keeps the hang count.
        if outcome == Outcome::Missed {
            self.unanswered.store(0, Ordering::Relaxed);
        }
        self.note(outcome);
    }

    /// Whether the block sent with [`PluginAudio::send`] has been answered, waiting for it at
    /// most until `until`, without taking it ([`Link::wait_answered`]): after `true`,
    /// [`PluginAudio::finish`] takes the result without waiting. Counts nothing. Real-time safe.
    pub fn poll(&self, until: Instant) -> bool {
        self.link.wait_answered(until)
    }

    /// Whether a block can go now ([`Link::ready`]), waiting at most until `until` (a time
    /// already past only looks) for the host to finish the block it was late with, so the next
    /// one goes as soon as it can rather than being refused (#506). Only a host late with the
    /// last block alone is waited for: one that has not answered several (hung, or far behind)
    /// is not, and its blocks are refused at once as before. Counts nothing. Real-time safe.
    pub fn ready(&self, until: Instant) -> bool {
        self.link.ready() || (self.unanswered.load(Ordering::Relaxed) <= 1 && self.link.wait_ready(until))
    }

    /// The second half: waits at most `timeout` for the block sent with [`PluginAudio::send`]
    /// and reads its result into `output`. Real-time safe.
    pub fn finish(&mut self, output: &mut [f32], timeout: Duration) -> Outcome {
        let outcome = self.link.wait(output, Instant::now() + timeout);
        self.note(outcome);
        outcome
    }

    fn note(&self, outcome: Outcome) {
        if outcome == Outcome::Done {
            self.unanswered.store(0, Ordering::Relaxed);
        } else {
            self.unanswered.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A plug-in's saved state: the component's (CLAP and VST2 have only this), and a VST3
/// plug-in's controller state when it keeps one apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedState {
    pub data: Vec<u8>,
    pub controller: Option<Vec<u8>>,
}

impl From<Vec<u8>> for SavedState {
    fn from(data: Vec<u8>) -> Self {
        Self { data, controller: None }
    }
}

/// Locks `mutex`, taking it over from a thread that panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `mutex`, waiting for it until `deadline` and giving up after that.
fn lock_until<T>(mutex: &Mutex<T>, deadline: Instant) -> Option<MutexGuard<'_, T>> {
    loop {
        if let Some(guard) = try_lock(mutex) {
            return Some(guard);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// `mutex` if nobody holds it now.
fn try_lock<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// A running host process and its control connection.
struct HostProcess {
    child: Child,
    /// Behind a lock of its own, held only while a line is written: an editor's placement
    /// ([`EditorPlacer`]) goes out while another request waits for its answer.
    control: std::sync::Arc<Mutex<Stream>>,
    replies: Receiver<Message>,
}

/// Sends an embedded editor's placement and size to its host, without the plug-in's control
/// half and without waiting for any request the host is busy with: the editor follows the
/// window at once, also while the plug-in is being edited or opened.
#[derive(Clone)]
pub struct EditorPlacer {
    writer: std::sync::Arc<Mutex<Stream>>,
    instance: u32,
}

impl EditorPlacer {
    fn send(&self, message: &Message) -> bool {
        let mut line = message.encode();
        line.push('\n');
        let mut writer = lock(&self.writer);
        writer.write_all(line.as_bytes()).and_then(|()| writer.flush()).is_ok()
    }

    /// Puts the embedded editor where `placement` says; `false` when the host is gone.
    pub fn place(&self, placement: Placement) -> bool {
        self.send(&Message::PlaceEditor { instance: self.instance, placement })
    }

    /// What the title bar of the editor's own window says; `false` when the host is gone.
    pub fn title(&self, title: &str) -> bool {
        self.send(&Message::EditorTitle { instance: self.instance, title: title.to_string() })
    }

    /// Asks the embedded editor to take `width` × `height` pixels; `false` when the host is gone.
    pub fn fit(&self, width: u32, height: u32) -> bool {
        self.send(&Message::FitEditor { instance: self.instance, width, height })
    }
}

impl HostProcess {
    /// Starts `launch.host` and says hello.
    fn start(launch: &Launch) -> Result<Self, PluginError> {
        let (address, name) = control_name()?;
        let listener = ListenerOptions::new().name(name).nonblocking(ListenerNonblockingMode::Accept).create_sync()?;
        let mut child = host_command(&launch.host)
            .args(["--control", &address])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        let began = Instant::now();
        crate::trace!("host process started");
        let deadline = Instant::now() + launch.timeout;
        let control = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e.into());
                }
            }
            if child.try_wait()?.is_some() {
                return Err(PluginError::Protocol("the host exited while starting".into()));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PluginError::Timeout("start"));
            }
            thread::sleep(Duration::from_millis(1));
        };
        crate::trace!("host process connected after {:?}", began.elapsed());
        control.set_nonblocking(false)?;
        let replies = read_lines(control.try_clone()?);
        Ok(Self { child, control: std::sync::Arc::new(Mutex::new(control)), replies })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, message: &Message) -> Result<(), PluginError> {
        let mut line = message.encode();
        line.push('\n');
        let mut control = lock(&self.control);
        control.write_all(line.as_bytes())?;
        control.flush()?;
        Ok(())
    }

    /// Ends the process, whatever its state.
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Asks the process to quit, gives it a moment, then ends it.
    fn quit(mut self) {
        let _ = self.send(&Message::Quit);
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        self.kill();
    }
}

/// The host process a group of instances share (or one instance alone), and which of the
/// processes started for the group it is.
struct Group {
    process: Option<HostProcess>,
    /// Goes up with every process started for the group: an instance loaded into an earlier
    /// one has lost its process. Taken from a counter all groups share, so it also tells
    /// processes apart: instances with the same generation run in the same process.
    generation: u64,
    /// Instances whose editor the host said was closed, not yet taken in by them.
    closed_editors: Vec<u32>,
    /// Instances whose editor the host said got a new size (the latest), not yet taken in.
    resized_editors: Vec<(u32, (u32, u32))>,
    /// Instances whose plug-in reported another latency (the latest), not yet taken in.
    latencies: Vec<(u32, u32)>,
    /// Parameters the host said the user moved in a plug-in's editor (the latest per instance
    /// and id), not yet taken in.
    param_edits: Vec<(u32, u32, f64)>,
    /// The notes ARA plug-ins found in their regions (the latest per instance and region), not
    /// yet taken in (#720).
    ara_notes: Vec<(u32, String, Vec<protocol::AraNote>)>,
    /// How far each ARA instance's analysis has come, as last said, not yet taken in.
    ara_analysis: Vec<(u32, Option<f32>)>,
    /// The process's resident memory, in bytes, the host last reported; gone once the
    /// process ends, so a dead host is never counted.
    ram: Option<u64>,
    next_ping: u64,
}

impl Group {
    fn new() -> Self {
        Self {
            process: None,
            generation: 0,
            closed_editors: Vec::new(),
            resized_editors: Vec::new(),
            latencies: Vec::new(),
            param_edits: Vec::new(),
            ara_notes: Vec::new(),
            ara_analysis: Vec::new(),
            ram: None,
            next_ping: 0,
        }
    }

    fn alive(&mut self) -> bool {
        self.process.as_mut().is_some_and(HostProcess::alive)
    }

    /// Ends the current process, if any, whatever its state.
    fn end(&mut self) {
        if let Some(process) = self.process.take() {
            process.kill();
        }
        self.closed_editors.clear();
        self.resized_editors.clear();
        self.latencies.clear();
        self.param_edits.clear();
        self.ara_notes.clear();
        self.ram = None;
    }

    /// Ends the current process and starts a new one for the group.
    fn spawn(&mut self, launch: &Launch) -> Result<(), PluginError> {
        self.end();
        self.generation = next_generation();
        self.process = Some(HostProcess::start(launch)?);
        let hello = self.request(None, &Message::Hello { version: VERSION }, "hello", launch.timeout, |m| {
            matches!(m, Message::Hello { .. })
        });
        match hello {
            Ok(Message::Hello { version }) if version == VERSION => Ok(()),
            Ok(other) => {
                self.end();
                Err(PluginError::Protocol(format!("host speaks another version: {other:?}")))
            }
            Err(e) => {
                self.end();
                Err(e)
            }
        }
    }

    /// Takes in what a message the host sent unasked says.
    fn note(&mut self, message: &Message) {
        match message {
            Message::EditorClosed { instance } => {
                if !self.closed_editors.contains(instance) && self.closed_editors.len() < MAX_CLOSED_EDITORS {
                    self.closed_editors.push(*instance);
                }
            }
            Message::EditorResized { instance, width, height } => {
                self.resized_editors.retain(|(i, _)| i != instance);
                if self.resized_editors.len() < MAX_CLOSED_EDITORS {
                    self.resized_editors.push((*instance, (*width, *height)));
                }
            }
            Message::LatencyChanged { instance, latency } => {
                self.latencies.retain(|(i, _)| i != instance);
                if self.latencies.len() < MAX_CLOSED_EDITORS {
                    self.latencies.push((*instance, *latency));
                }
            }
            Message::ParamChanged { instance, id, value } => {
                // The last value of a parameter wins; one that already moved a step keeps its
                // latest.
                self.param_edits.retain(|(i, other, _)| !(i == instance && other == id));
                if self.param_edits.len() < MAX_CLOSED_EDITORS {
                    self.param_edits.push((*instance, *id, *value));
                }
            }
            Message::AraNotes { instance, region, notes, .. } => {
                self.ara_notes.retain(|(i, r, _)| !(i == instance && r == region));
                if self.ara_notes.len() < protocol::MAX_ARA_OBJECTS {
                    self.ara_notes.push((*instance, region.clone(), notes.clone()));
                }
            }
            Message::AraAnalysis { instance, progress } => {
                self.ara_analysis.retain(|(i, _)| i != instance);
                if self.ara_analysis.len() < MAX_CLOSED_EDITORS {
                    self.ara_analysis.push((*instance, *progress));
                }
            }
            Message::TransportKey | Message::TypedKey { .. } | Message::AraPlayback(_) => take_in_transport(message),
            Message::Memory { bytes } => {
                // Only a process that is still there reports; the host sends it once a second,
                // so the latest is what the control bar adds.
                self.ram = Some(*bytes);
            }
            _ => {}
        }
    }

    /// Takes in the messages waiting, without waiting for more.
    fn drain(&mut self) {
        let mut waiting = Vec::new();
        if let Some(process) = &self.process {
            while let Ok(message) = process.replies.try_recv() {
                waiting.push(message);
            }
        }
        for message in &waiting {
            self.note(message);
        }
    }

    /// Whether the host said `instance`'s editor was closed (and forgets it).
    fn take_closed(&mut self, instance: u32) -> bool {
        let before = self.closed_editors.len();
        self.closed_editors.retain(|i| *i != instance);
        self.closed_editors.len() != before
    }

    /// The size the host said `instance`'s editor got, if any (and forgets it).
    fn take_resized(&mut self, instance: u32) -> Option<(u32, u32)> {
        let at = self.resized_editors.iter().position(|(i, _)| *i == instance)?;
        Some(self.resized_editors.swap_remove(at).1)
    }

    /// How far `instance`'s analysis has come, when the host said so since the last call.
    fn take_ara_analysis(&mut self, instance: u32) -> Option<Option<f32>> {
        let at = self.ara_analysis.iter().position(|(i, _)| *i == instance)?;
        Some(self.ara_analysis.swap_remove(at).1)
    }

    /// The notes the host sent for `instance`'s regions since the last call (and forgets them).
    fn take_ara_notes(&mut self, instance: u32) -> Vec<(String, Vec<protocol::AraNote>)> {
        let mut taken = Vec::new();
        self.ara_notes.retain_mut(|(i, region, notes)| {
            if *i != instance {
                return true;
            }
            taken.push((std::mem::take(region), std::mem::take(notes)));
            false
        });
        taken
    }

    /// The latency the host said `instance`'s plug-in now has, if any (and forgets it).
    fn take_latency(&mut self, instance: u32) -> Option<u32> {
        let at = self.latencies.iter().position(|(i, _)| *i == instance)?;
        Some(self.latencies.swap_remove(at).1)
    }

    /// What the host said the user moved in `instance`'s editor since the last call (and
    /// forgets it): each (id, value) once, the latest per id.
    fn take_param_edits(&mut self, instance: u32) -> Vec<(u32, f64)> {
        let mut taken = Vec::new();
        self.param_edits.retain_mut(|(i, id, value)| {
            if *i != instance {
                return true;
            }
            taken.push((*id, *value));
            false
        });
        taken
    }

    /// Sends `message` (about `instance`, or the process) and waits for the reply `wanted`
    /// accepts, skipping stale ones and noting unasked ones; an `error` about the same
    /// instance (or the process) fails.
    fn request(
        &mut self,
        instance: Option<u32>,
        message: &Message,
        what: &'static str,
        timeout: Duration,
        wanted: impl Fn(&Message) -> bool,
    ) -> Result<Message, PluginError> {
        let process = self.process.as_mut().ok_or_else(|| PluginError::Protocol("the host is not running".into()))?;
        process.send(message)?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let received = match &self.process {
                Some(process) => process.replies.recv_timeout(left),
                None => Err(RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Message::Error { instance: about, message }) if about.is_none() || about == instance => {
                    return Err(PluginError::Host(message));
                }
                Ok(message) if wanted(&message) => return Ok(message),
                Ok(other) => self.note(&other),
                Err(RecvTimeoutError::Timeout) => return Err(PluginError::Timeout(what)),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(PluginError::Protocol("the host closed the control connection".into()));
                }
            }
        }
    }

    /// Whether the process's control thread answers within `timeout`.
    fn ping(&mut self, timeout: Duration) -> bool {
        self.next_ping += 1;
        let id = self.next_ping;
        self.request(None, &Message::Ping { id }, "ping", timeout, |m| *m == Message::Pong { id }).is_ok()
    }

    /// Loads `instance` as `launch` says into the group's process, starting one if none runs.
    /// Returns the generation it was loaded into, its latency and its link.
    fn load(&mut self, launch: &Launch, instance: u32) -> Result<(u64, u32, bool, Link), PluginError> {
        let mut link = Link::create(launch.channels, launch.max_frames)?;
        if !self.alive() {
            self.spawn(launch)?;
        }
        let load = Message::Load {
            instance,
            plugin: launch.plugin.clone(),
            link: link.address(),
            sample_rate: launch.sample_rate,
            ara: launch.ara.clone(),
        };
        let loaded = self.request(
            Some(instance),
            &load,
            "load",
            launch.timeout,
            |m| matches!(m, Message::Loaded { instance: i, .. } if *i == instance),
        )?;
        let (latency, edits_clips) = match loaded {
            Message::Loaded { latency, edits_clips, .. } => (latency, edits_clips),
            _ => (0, false),
        };
        // The host opened the link before it answered, and never opens it again.
        link.host_opened();
        Ok((self.generation, latency, edits_clips, link))
    }
}

impl Drop for Group {
    /// The last instance of the group is gone: the process quits.
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            process.quit();
        }
    }
}

/// A group's key: the host executable and the group's name.
type GroupKey = (PathBuf, String);

/// The groups whose process is shared, while any instance in them lives.
static GROUPS: Mutex<BTreeMap<GroupKey, Weak<Mutex<Group>>>> = Mutex::new(BTreeMap::new());

/// The group `launch` asks for: the shared one for its host and group name, or one of its own.
fn group_for_launch(launch: &Launch) -> Arc<Mutex<Group>> {
    let mut groups = lock(&GROUPS);
    groups.retain(|_, group| group.strong_count() > 0);
    let Some(name) = &launch.group else {
        // A group of its own is still one host process whose RAM the control bar counts, so it
        // is tracked like the named ones, under a key no other group takes. The last instance
        // dropping its half drops the entry with it (the register keeps weak references).
        static UNNAMED: AtomicU64 = AtomicU64::new(0);
        let key = (launch.host.clone(), format!("instance:{}", UNNAMED.fetch_add(1, Ordering::Relaxed)));
        let group = Arc::new(Mutex::new(Group::new()));
        groups.insert(key, Arc::downgrade(&group));
        return group;
    };
    let key = (launch.host.clone(), name.clone());
    if let Some(group) = groups.get(&key).and_then(Weak::upgrade) {
        return group;
    }
    let group = Arc::new(Mutex::new(Group::new()));
    groups.insert(key, Arc::downgrade(&group));
    group
}

/// A fresh generation for a group's new process, unique in this process.
fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A fresh instance number, unique in this process.
fn next_instance() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The control half: one plug-in instance in a host process.
pub struct Plugin {
    /// Shared with the retained audio half so a later crash restart uses its current rate.
    sample_rate: Arc<AtomicU64>,
    launch: Launch,
    group: Arc<Mutex<Group>>,
    instance: u32,
    /// The group's generation this instance was loaded into.
    generation: u64,
    unanswered: Arc<AtomicU32>,
    latency: u32,
    /// The plug-in supports ARA, as the host found when it loaded it.
    edits_clips: bool,
    /// The state last read from the plug-in; loaded again after a restart.
    last_state: Option<SavedState>,
    /// Parameters set since the state was last read or loaded, in order; set again after a
    /// restart, on top of that state.
    params_since: Vec<(u32, f64)>,
    /// Whether the plug-in's editor window is open.
    editor_open: bool,
    /// The size of the open editor's inside, as the host last said.
    editor_size: Option<(u32, u32)>,
    /// What [`Plugin::health`] last found, for when the process is busy with a sibling.
    last_health: Health,
}

impl Plugin {
    /// Loads the plug-in into a host process and returns both halves: into the running process
    /// of its group ([`Launch::group`]), or a new one when the group has none running or the
    /// instance has no group.
    pub fn start(launch: Launch) -> Result<(Self, PluginAudio), PluginError> {
        let group = group_for_launch(&launch);
        let instance = next_instance();
        let (generation, latency, edits_clips, link) = lock(&group).load(&launch, instance)?;
        let unanswered = Arc::new(AtomicU32::new(0));
        let sample_rate = Arc::new(AtomicU64::new(launch.sample_rate.to_bits()));
        let audio_group = Arc::downgrade(&group);
        let plugin = Self {
            sample_rate: Arc::clone(&sample_rate),
            launch,
            group,
            instance,
            generation,
            unanswered: unanswered.clone(),
            latency,
            edits_clips,
            last_state: None,
            params_since: Vec::new(),
            editor_open: false,
            editor_size: None,
            last_health: Health::Running,
        };
        Ok((plugin, PluginAudio { link, unanswered, instance, process: generation, group: audio_group, sample_rate }))
    }

    /// Delay the plug-in adds, in frames, as it last reported: when it loaded, or since (see
    /// [`Plugin::follow_latency`]).
    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// Whether the plug-in supports ARA (it can edit clips, ADR 0033), as the host found when
    /// it loaded it: true even when the scan did not see it, or it was loaded without a document.
    pub fn edits_clips(&self) -> bool {
        self.edits_clips
    }

    /// Takes in a latency the plug-in reported while it runs (a lookahead it was given, say:
    /// the host sends it unasked) and returns the latency as it is now. Never waits: while the
    /// process is busy with another request, the answer is the last one known.
    pub fn follow_latency(&mut self) -> u32 {
        if let Some(mut group) = try_lock(&self.group)
            && group.generation == self.generation
        {
            group.drain();
            if let Some(latency) = group.take_latency(self.instance) {
                self.latency = latency;
            }
        }
        self.latency
    }

    /// What the user moved in the plug-in's editor since the last call, as (id, value) pairs
    /// in the plug-in's own range (see [`ParamInfo`]): the session keeps the project's values
    /// in step with them. Never waits: while the process is busy with another request,
    /// nothing yet.
    pub fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        let Some(mut group) = try_lock(&self.group) else { return Vec::new() };
        if group.generation != self.generation {
            return Vec::new();
        }
        group.drain();
        let taken = group.take_param_edits(self.instance);
        // An edit the session keeps the project's values in step with is also a value set
        // since the state was last read: a restart sets it again, the latest of a parameter
        // winning (ADR 0006, decision 3). Kept only until the state is read again, which
        // holds it.
        for (id, value) in &taken {
            self.params_since.retain(|(other, _)| *other != *id);
            if self.params_since.len() < protocol::MAX_PARAMS {
                self.params_since.push((*id, *value));
            }
        }
        taken
    }

    /// The notes the ARA plug-in found in its regions since the last call, by region id
    /// (`clip-<id>`): each the region's notes as they are now, empty when it has none (#720).
    /// Never waits: while the process is busy with another request, nothing yet.
    pub fn take_ara_analysis(&mut self) -> Option<Option<f32>> {
        let mut group = try_lock(&self.group)?;
        if group.generation != self.generation {
            return None;
        }
        group.drain();
        group.take_ara_analysis(self.instance)
    }

    pub fn take_ara_notes(&mut self) -> Vec<(String, Vec<protocol::AraNote>)> {
        let Some(mut group) = try_lock(&self.group) else { return Vec::new() };
        if group.generation != self.generation {
            return Vec::new();
        }
        group.drain();
        group.take_ara_notes(self.instance)
    }

    /// The host process's id (0 when it has none running), for tests and diagnostics.
    pub fn process_id(&self) -> u32 {
        lock(&self.group).process.as_ref().map_or(0, |p| p.child.id())
    }

    /// How the plug-in is. Never waits: while its process is busy with another request (a
    /// sibling loading, which can take seconds), it says what it found last time, so a window
    /// that asks every frame does not stop.
    pub fn health(&mut self) -> Health {
        let Some(mut group) = try_lock(&self.group) else {
            if self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER {
                return Health::Hung;
            }
            return self.last_health;
        };
        self.last_health = if group.generation != self.generation || !group.alive() {
            Health::Crashed
        } else if self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER {
            Health::Hung
        } else {
            Health::Running
        };
        self.last_health
    }

    /// Whether the host's control thread answers within the launch timeout.
    pub fn ping(&mut self) -> bool {
        let mut group = lock(&self.group);
        group.generation == self.generation && group.ping(self.launch.timeout)
    }

    /// Sends a request about this instance and waits for the reply `wanted` accepts.
    fn request(
        &mut self,
        message: &Message,
        what: &'static str,
        wanted: impl Fn(&Message) -> bool,
    ) -> Result<Message, PluginError> {
        self.request_in(None, message, what, wanted)
    }

    /// [`Plugin::request`], waiting at most `timeout` for the group's lock and the reply together,
    /// so a sibling in the same host process that is busy cannot hold the caller longer.
    fn request_within(
        &mut self,
        timeout: Duration,
        message: &Message,
        what: &'static str,
        wanted: impl Fn(&Message) -> bool,
    ) -> Result<Message, PluginError> {
        self.request_in(Some(timeout), message, what, wanted)
    }

    /// The request itself: `within` bounds the wait for the lock and the reply, `None` waits for
    /// the lock as long as it takes and for the reply as long as the launch's timeout.
    fn request_in(
        &mut self,
        within: Option<Duration>,
        message: &Message,
        what: &'static str,
        wanted: impl Fn(&Message) -> bool,
    ) -> Result<Message, PluginError> {
        let (mut group, timeout) = match within {
            None => (lock(&self.group), self.launch.timeout),
            Some(within) => {
                let deadline = Instant::now() + within;
                let group = lock_until(&self.group, deadline).ok_or(PluginError::Timeout(what))?;
                (group, deadline.saturating_duration_since(Instant::now()))
            }
        };
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let reply = group.request(Some(self.instance), message, what, timeout, wanted);
        if let Some(latency) = group.take_latency(self.instance) {
            self.latency = latency;
        }
        if let Some(size) = group.take_resized(self.instance) {
            self.editor_size = Some(size);
        }
        if group.take_closed(self.instance) {
            self.editor_open = false;
            self.editor_size = None;
        }
        reply
    }

    /// The state last read from or given to the plug-in, without asking it.
    pub fn last_state(&self) -> Option<&SavedState> {
        self.last_state.as_ref()
    }

    /// Reads the plug-in's state and keeps it for a restart.
    pub fn save_state(&mut self) -> Result<SavedState, PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::GetState { instance },
            "state",
            |m| matches!(m, Message::State { instance: i, .. } if *i == instance),
        )?;
        let Message::State { data, controller, .. } = reply else {
            return Err(PluginError::Protocol("expected state".into()));
        };
        let state = SavedState { data, controller };
        self.last_state = Some(state.clone());
        self.params_since.clear();
        Ok(state)
    }

    /// Hands `state` to the plug-in (and keeps it for a restart). A plug-in that does not take
    /// it answers with an error and keeps the state it had.
    pub fn load_state(&mut self, state: impl Into<SavedState>) -> Result<(), PluginError> {
        let state = state.into();
        let instance = self.instance;
        let message = Message::SetState { instance, data: state.data.clone(), controller: state.controller.clone() };
        self.request(&message, "set_state", |m| matches!(m, Message::State { instance: i, .. } if *i == instance))?;
        self.last_state = Some(state);
        self.params_since.clear();
        Ok(())
    }

    /// The plug-in's parameters with their current values.
    pub fn params(&mut self) -> Result<Vec<ParamInfo>, PluginError> {
        self.params_within(self.launch.timeout)
    }

    /// Changes the audio rate without replacing the instance or its editor. The caller
    /// must stop submitting audio first. A failed request requires explicit recovery.
    pub fn reconfigure(&mut self, sample_rate: f64, timeout: Duration) -> Result<u32, PluginError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || sample_rate > 768_000.0 {
            return Err(PluginError::Protocol("invalid sample rate".into()));
        }
        let instance = self.instance;
        let reply = self.request_within(
            timeout,
            &Message::Reconfigure { instance, sample_rate },
            "reconfigure",
            |m| matches!(m, Message::Reconfigured { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::Reconfigured { latency, .. } => {
                self.launch.sample_rate = sample_rate;
                self.sample_rate.store(sample_rate.to_bits(), Ordering::Release);
                self.latency = latency;
                Ok(latency)
            }
            _ => Err(PluginError::Protocol("expected reconfiguration acknowledgement".into())),
        }
    }

    /// [`Plugin::params`], waiting at most `timeout` for the host's answer instead of the launch
    /// timeout, for a caller that must not keep the plug-in's control half for long. A late
    /// answer is skipped by the next request.
    pub fn params_within(&mut self, timeout: Duration) -> Result<Vec<ParamInfo>, PluginError> {
        let instance = self.instance;
        let reply = self.request_within(
            timeout,
            &Message::GetParams { instance },
            "params",
            |m| matches!(m, Message::Params { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::Params { params, .. } => Ok(params),
            _ => Err(PluginError::Protocol("expected parameters".into())),
        }
    }

    /// Sets parameter `id` to `value`, in its own range (see [`ParamInfo`]); the plug-in's
    /// audio follows from its next block. Returns the parameter as it is now. Kept for a
    /// restart.
    pub fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::SetParam { instance, id, value },
            "set_param",
            |m| matches!(m, Message::Param { instance: i, .. } if *i == instance),
        )?;
        let Message::Param { param, .. } = reply else {
            return Err(PluginError::Protocol("expected a parameter".into()));
        };
        self.params_since.retain(|(other, _)| *other != id);
        if self.params_since.len() < protocol::MAX_PARAMS {
            self.params_since.push((id, param.value));
        }
        Ok(param)
    }

    /// Opens the plug-in's editor in a window of its own (in the host process) and returns the
    /// size of its inside. Opening an open editor just returns its size.
    pub fn open_editor(&mut self) -> Result<(u32, u32), PluginError> {
        self.open_editor_owned(None)
    }

    /// [`Plugin::open_editor`] in a window that `owner` (the engine's window: a Win32 `HWND`,
    /// an X11 window id) owns: it floats above it and goes with it.
    pub fn open_editor_owned(&mut self, owner: Option<u64>) -> Result<(u32, u32), PluginError> {
        self.open_editor_in(owner, false, None)
    }

    /// [`Plugin::open_editor_owned`], its window saying `title` from the moment it opens.
    pub fn open_editor_titled(&mut self, owner: Option<u64>, title: &str) -> Result<(u32, u32), PluginError> {
        self.open_editor_in(owner, false, Some(title.to_string()))
    }

    /// [`Plugin::open_editor`] as a child inside the engine's window `owner` (a Win32 `HWND`,
    /// an X11 window id), hidden until [`Plugin::place_editor`] puts it somewhere in it.
    pub fn open_editor_embedded(&mut self, owner: u64) -> Result<(u32, u32), PluginError> {
        self.open_editor_in(Some(owner), true, None)
    }

    fn open_editor_in(
        &mut self,
        owner: Option<u64>,
        embedded: bool,
        title: Option<String>,
    ) -> Result<(u32, u32), PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::OpenEditor { instance, owner, embedded, title },
            "open_editor",
            |m| matches!(m, Message::EditorOpened { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::EditorOpened { width, height, .. } => {
                self.editor_open = true;
                self.editor_size = Some((width, height));
                Ok((width, height))
            }
            _ => Err(PluginError::Protocol("expected the editor".into())),
        }
    }

    /// Puts an embedded editor at `placement` in the engine's window. Never waits for the
    /// host, and not for the process either: while it is busy with another request the
    /// placement is not sent, and this returns `false` so the caller sends it again later.
    /// What sends this instance's embedded editor its placement and size without the control
    /// half ([`EditorPlacer`]). `None` while the process is busy with another request, or gone.
    pub fn placer(&mut self) -> Option<EditorPlacer> {
        let group = try_lock(&self.group)?;
        if group.generation != self.generation {
            return None;
        }
        let process = group.process.as_ref()?;
        Some(EditorPlacer { writer: process.control.clone(), instance: self.instance })
    }

    pub fn place_editor(&mut self, placement: Placement) -> Result<bool, PluginError> {
        let Some(mut group) = try_lock(&self.group) else { return Ok(false) };
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let process = group.process.as_mut().ok_or_else(|| PluginError::Protocol("the host is not running".into()))?;
        process.send(&Message::PlaceEditor { instance: self.instance, placement })?;
        Ok(true)
    }

    /// Asks an embedded editor to take `width` × `height` pixels, the room the engine has for
    /// it. Never waits, as [`Plugin::place_editor`]: `false` when it was not sent.
    pub fn fit_editor(&mut self, width: u32, height: u32) -> Result<bool, PluginError> {
        let Some(mut group) = try_lock(&self.group) else { return Ok(false) };
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let process = group.process.as_mut().ok_or_else(|| PluginError::Protocol("the host is not running".into()))?;
        process.send(&Message::FitEditor { instance: self.instance, width, height })?;
        Ok(true)
    }

    /// Brings the ARA document of a plug-in loaded with one in line with `document`, its
    /// track's clips as they are now (moved, trimmed, added or removed), without loading it
    /// again. Waits for the answer.
    pub fn ara_update(&mut self, document: AraDocument) -> Result<(), PluginError> {
        let instance = self.instance;
        self.request(
            &Message::AraUpdate { instance, document },
            "ara_update",
            |m| matches!(m, Message::AraUpdated { instance: i } if *i == instance),
        )?;
        Ok(())
    }

    /// The edits of one modification (`content-<id>`) of a plug-in bound to an ARA document and
    /// its source (the pool key), stored on their own to give another track's plug-in (ADR 0033
    /// step 6): the plug-in's document archive id and the bytes. Waits for the answer.
    pub fn ara_export(&mut self, modification: &str, source: &str) -> Result<(String, Vec<u8>), PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::AraExport { instance, modification: modification.to_string(), source: source.to_string() },
            "ara_export",
            |m| matches!(m, Message::AraExported { instance: i, .. } if *i == instance),
        )?;
        match reply {
            Message::AraExported { archive_id, data, .. } => Ok((archive_id, data)),
            _ => Err(PluginError::Protocol("expected the exported edits".into())),
        }
    }

    /// Gives a plug-in bound to an ARA document edits another track's stored with
    /// [`Plugin::ara_export`], for its modification `to` (it was `from` there) of `source`.
    /// Fails while its document has no `to` yet. Waits for the answer.
    pub fn ara_import(
        &mut self,
        (archive_id, data): (&str, &[u8]),
        from: &str,
        to: &str,
        source: &str,
    ) -> Result<(), PluginError> {
        let instance = self.instance;
        let message = Message::AraImport {
            instance,
            archive_id: archive_id.to_string(),
            data: data.to_vec(),
            from: from.to_string(),
            to: to.to_string(),
            source: source.to_string(),
        };
        self.request(&message, "ara_import", |m| matches!(m, Message::AraImported { instance: i } if *i == instance))?;
        Ok(())
    }

    /// Tells a plug-in bound to an ARA document which region (`clip-<id>`) the engine has
    /// selected, or none. Never waits, as [`Plugin::place_editor`]: returns `false` when the
    /// host process was busy and it was not sent, so the caller sends it again later.
    pub fn ara_select(&mut self, region: Option<&str>) -> Result<bool, PluginError> {
        let Some(mut group) = try_lock(&self.group) else { return Ok(false) };
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let process = group.process.as_mut().ok_or_else(|| PluginError::Protocol("the host is not running".into()))?;
        process.send(&Message::AraSelect { instance: self.instance, region: region.map(str::to_string) })?;
        Ok(true)
    }

    /// Closes the plug-in's editor, if it is open.
    pub fn close_editor(&mut self) -> Result<(), PluginError> {
        let instance = self.instance;
        self.request(
            &Message::CloseEditor { instance },
            "close_editor",
            |m| matches!(m, Message::EditorClosed { instance: i } if *i == instance),
        )?;
        self.editor_open = false;
        self.editor_size = None;
        Ok(())
    }

    /// Whether the editor is open: the user may have closed its window since it was opened.
    /// Never waits: while the process is busy with another request, the answer is the last
    /// one known.
    pub fn editor_open(&mut self) -> bool {
        self.take_editor_news();
        self.editor_open
    }

    /// The size of the open editor's inside, as the host last said (the plug-in may have
    /// changed it since it opened); `None` while it is closed. Never waits, like
    /// [`Plugin::editor_open`].
    pub fn editor_size(&mut self) -> Option<(u32, u32)> {
        self.take_editor_news();
        self.editor_size
    }

    /// Takes in what the host said unasked about the editor, if the process is not busy.
    fn take_editor_news(&mut self) {
        let Some(mut group) = try_lock(&self.group) else { return };
        if group.generation != self.generation {
            self.editor_open = false;
            self.editor_size = None;
            return;
        }
        group.drain();
        if let Some(latency) = group.take_latency(self.instance) {
            self.latency = latency;
        }
        if let Some(size) = group.take_resized(self.instance) {
            self.editor_size = Some(size);
        }
        if group.take_closed(self.instance) {
            self.editor_open = false;
            self.editor_size = None;
        }
    }

    /// Brings the plug-in back with its last saved state and the parameters set since, and
    /// returns the new audio half, which replaces the old one.
    ///
    /// When the process is gone, does not answer, or this instance hung in it (its audio
    /// thread is stuck there, so it cannot be let go of), the process is ended and a new one
    /// started for the group; the other instances in it then report [`Health::Crashed`], and
    /// their own restart loads them into the new process. An instance whose process was
    /// already replaced by another's restart is loaded into the new one. An instance with a
    /// process of its own always gets a new process.
    pub fn restart(&mut self) -> Result<PluginAudio, PluginError> {
        self.launch.sample_rate = f64::from_bits(self.sample_rate.load(Ordering::Acquire));
        // The instance being replaced: if its audio thread is stuck in a shared process, that
        // process cannot let it go and is ended. Read before the counter is replaced below.
        let own_hung = self.unanswered.load(Ordering::Relaxed) >= HUNG_AFTER;
        let shared = self.launch.group.is_some();
        let group = self.group.clone();
        let mut locked = lock(&group);
        let current = locked.generation == self.generation;
        let alive = locked.alive();
        if !shared || !alive || (current && (own_hung || !locked.ping(self.launch.timeout))) {
            locked.end();
        } else if current {
            let instance = self.instance;
            let unload = Message::Unload { instance };
            let unloaded = locked.request(
                Some(instance),
                &unload,
                "unload",
                self.launch.timeout.min(UNLOAD_TIMEOUT),
                |m| matches!(m, Message::Unloaded { instance: i } if *i == instance),
            );
            locked.take_closed(instance);
            locked.take_latency(instance);
            if matches!(unloaded, Err(PluginError::Timeout(_))) {
                // Stuck in the old instance's teardown: the new one starts in a new process.
                locked.end();
            }
        }
        let instance = next_instance();
        let (generation, latency, edits_clips, link) = locked.load(&self.launch, instance)?;
        drop(locked);
        self.edits_clips = edits_clips;
        self.instance = instance;
        self.generation = generation;
        self.latency = latency;
        self.editor_open = false;
        self.editor_size = None;
        self.unanswered = Arc::new(AtomicU32::new(0));
        // What `health` says while the group is busy (#188): the new instance, not the old one's
        // `Crashed` or `Hung`.
        self.last_health = Health::Running;
        let audio = PluginAudio {
            link,
            unanswered: self.unanswered.clone(),
            instance,
            process: generation,
            group: Arc::downgrade(&self.group),
            sample_rate: Arc::clone(&self.sample_rate),
        };
        let params = std::mem::take(&mut self.params_since);
        if let Some(state) = self.last_state.clone() {
            self.load_state(state)?;
        }
        for (id, value) in params {
            self.set_param(id, value)?;
        }
        Ok(audio)
    }
}

impl Drop for Plugin {
    /// Lets go of the instance; the process quits when its last instance is gone.
    fn drop(&mut self) {
        let mut group = lock(&self.group);
        if self.launch.group.is_none() || group.generation != self.generation || !group.alive() {
            return;
        }
        let instance = self.instance;
        let unloaded = group.request(
            Some(instance),
            &Message::Unload { instance },
            "unload",
            self.launch.timeout.min(UNLOAD_TIMEOUT),
            |m| matches!(m, Message::Unloaded { instance: i } if *i == instance),
        );
        group.take_closed(instance);
        group.take_latency(instance);
        if matches!(unloaded, Err(PluginError::Timeout(_))) {
            // A process that does not answer is no use to the other instances either.
            group.end();
        }
    }
}

/// A command that runs the plug-in host executable `host` without a window of its own: on
/// Windows a console program started from a program without a console (the desktop app) would
/// otherwise open a console window for every host and scan worker. Its own windows (plug-in
/// editors) still open.
pub fn host_command(host: &std::path::Path) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(host);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW` from the Windows process creation flags.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // A traced run's hosts trace to the same file (`docs/debugging.md`).
    if let Some(path) = trace::path() {
        command.env(trace::VARIABLE, path);
    }
    command
}

/// A fresh name for the control socket: the address to pass to the host, and the name itself.
fn control_name() -> Result<(String, Name<'static>), PluginError> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = format!("suhina-control-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    parse_address(&if GenericNamespaced::is_supported() {
        format!("ns:{id}")
    } else {
        format!("fs:{}", std::env::temp_dir().join(id).display())
    })
}

/// The control socket an address names: `ns:<name>` (a namespaced socket or pipe) or
/// `fs:<path>` (a socket file, where the system has no namespace).
pub fn parse_address(address: &str) -> Result<(String, Name<'static>), PluginError> {
    let bad = |e: std::io::Error| PluginError::Protocol(format!("bad control address {address:?}: {e}"));
    let name = if let Some(name) = address.strip_prefix("ns:") {
        name.to_string().to_ns_name::<GenericNamespaced>().map_err(bad)?
    } else if let Some(path) = address.strip_prefix("fs:") {
        PathBuf::from(path).to_fs_name::<GenericFilePath>().map_err(bad)?
    } else {
        return Err(PluginError::Protocol(format!("bad control address {address:?}")));
    };
    Ok((address.to_string(), name))
}

/// What ARA plug-ins asked of the transport and the window has not taken yet (#717).
static ARA_PLAYBACK: Mutex<Vec<protocol::AraPlayback>> = Mutex::new(Vec::new());
const MAX_ARA_PLAYBACK: usize = 16;

/// What ARA plug-ins asked of the transport since the last call, oldest first: play, stop, or a
/// play position in seconds. Takes in what the hosts sent without waiting for any of them.
pub fn take_ara_playback() -> Vec<protocol::AraPlayback> {
    drain_groups();
    std::mem::take(&mut *lock(&ARA_PLAYBACK))
}

/// Space presses the hosts' editor windows sent and the window has not taken yet.
static TRANSPORT_KEYS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// How many times Space was pressed in a plug-in's editor window since the last call: the window
/// plays or stops that many times, as Space in its own window does. Takes in what the hosts sent
/// without waiting for any of them.
/// Keys the app's computer keyboard plays, pressed or let go in plug-in editor windows (their
/// Windows virtual-key codes), oldest first.
static TYPED_KEYS: Mutex<Vec<(u8, bool)>> = Mutex::new(Vec::new());
/// At most this many wait; more are dropped (a stuck window would otherwise grow the list).
const MAX_TYPED_KEYS: usize = 64;

/// The keys pressed and let go in editor windows while the computer keyboard plays (#944).
pub fn take_typed_keys() -> Vec<(u8, bool)> {
    drain_groups();
    std::mem::take(&mut *lock(&TYPED_KEYS))
}

pub fn take_transport_keys() -> u32 {
    drain_groups();
    TRANSPORT_KEYS.swap(0, std::sync::atomic::Ordering::Relaxed)
}

/// Takes in what every host sent, without waiting for a group that is busy.
fn drain_groups() {
    let groups = lock(&GROUPS);
    for group in groups.values().filter_map(Weak::upgrade) {
        if let Some(mut group) = try_lock(&group) {
            group.drain();
        }
    }
}

/// The resident memory, in bytes, of every host process that has reported it and still runs,
/// once per process (the instances of one group share a process). Reads what the hosts sent;
/// never waits: a group busy with a sibling's load keeps its last report for next time. The
/// control bar adds these to the program's own RAM.
pub fn host_memory() -> Vec<u64> {
    let mut groups = lock(&GROUPS);
    groups.retain(|_, group| group.strong_count() > 0);
    groups
        .values()
        .filter_map(Weak::upgrade)
        .filter_map(|group| {
            let mut group = try_lock(&group)?;
            // What is waiting is taken in here and now, so an editor's events still reach the
            // instances on their next request.
            group.drain();
            // A process that has gone may have reported before it did; its report is stale.
            if group.alive() {
                group.ram
            } else {
                group.ram = None;
                None
            }
        })
        .collect()
}

/// What wakes the window when a host sends something it shows unasked (#714): Space in a
/// plug-in's window, an ARA plug-in's transport request or notes, an editor closed by hand.
type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;
static WAKER: Mutex<Option<Waker>> = Mutex::new(None);

/// Sets what wakes the window when a host sends something it shows unasked, so it answers at
/// once rather than on its next frame. The window gives its repaint request.
pub fn set_waker(waker: impl Fn() + Send + Sync + 'static) {
    *lock(&WAKER) = Some(std::sync::Arc::new(waker));
}

/// Takes in a transport request a host sent unasked: Space in an editor window, or an ARA
/// plug-in's play, stop or position. Done on the reader thread as the line comes, before the
/// window is woken, so the frame it wakes plays or moves at once: left in the replies until a
/// request read them, a click in the plug-in's timeline waited a frame or more, and the plug-in
/// drew the old position meanwhile.
fn take_in_transport(message: &Message) {
    match message {
        Message::TransportKey => {
            TRANSPORT_KEYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Message::TypedKey { key, pressed } => {
            let mut keys = lock(&TYPED_KEYS);
            if keys.len() < MAX_TYPED_KEYS {
                keys.push((*key, *pressed));
            }
        }
        Message::AraPlayback(request) => {
            let mut requests = lock(&ARA_PLAYBACK);
            // A newer position or loop replaces one not taken yet (a loop dragged in the editor
            // asks on every move); a few starts and stops are kept.
            match request {
                protocol::AraPlayback::Position(_) => {
                    requests.retain(|r| !matches!(r, protocol::AraPlayback::Position(_)));
                }
                protocol::AraPlayback::Cycle(..) => {
                    requests.retain(|r| !matches!(r, protocol::AraPlayback::Cycle(..)));
                }
                _ => {}
            }
            if requests.len() < MAX_ARA_PLAYBACK {
                requests.push(*request);
            }
        }
        _ => {}
    }
}

fn wake() {
    if let Some(waker) = lock(&WAKER).clone() {
        waker();
    }
}

/// Reads messages from `stream` on a thread of its own, so replies can be waited for with a
/// timeout. Unknown lines are skipped; the channel closes when the stream ends.
pub fn read_lines(stream: Stream) -> Receiver<Message> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match (&mut reader).take(MAX_LINE as u64 + 1).read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if let Some(message) = Message::decode(line.trim_end()) {
                        let shown = matches!(
                            message,
                            Message::TransportKey
                                | Message::TypedKey { .. }
                                | Message::AraPlayback(_)
                                | Message::AraNotes { .. }
                                | Message::AraAnalysis { .. }
                                | Message::EditorClosed { .. }
                        );
                        if matches!(message, Message::TransportKey | Message::TypedKey { .. } | Message::AraPlayback(_))
                        {
                            take_in_transport(&message);
                            wake();
                            continue;
                        }
                        if sender.send(message).is_err() {
                            return;
                        }
                        // After it is queued, so the frame it wakes takes it in.
                        if shown {
                            wake();
                        }
                    }
                }
            }
        }
    });
    receiver
}

/// Connects to the engine's control socket at `address` (the host side).
pub fn connect(address: &str) -> Result<Stream, PluginError> {
    let (_, name) = parse_address(address)?;
    Ok(Stream::connect(name)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions
    use super::*;
    use std::sync::mpsc::Sender;

    #[test]
    fn a_lock_waited_for_until_a_deadline_gives_up_after_it() {
        let mutex = Mutex::new(1);
        // Free: taken whatever the deadline.
        assert!(lock_until(&mutex, Instant::now()).is_some());
        // Held: a deadline already passed gives up at once, a later one is waited for.
        let held = lock(&mutex);
        assert!(lock_until(&mutex, Instant::now()).is_none());
        thread::scope(|scope| {
            let waiting = scope.spawn(|| lock_until(&mutex, Instant::now() + Duration::from_secs(60)).is_some());
            drop(held);
            assert!(waiting.join().unwrap());
        });
    }

    /// A host process that is alive but runs nothing: a real child and a control socket, with
    /// what the test `feed`s a group, so `host_memory` can be tried without the executable.
    fn living_process() -> (HostProcess, Sender<Message>) {
        let (address, name) = control_name().unwrap();
        let listener =
            ListenerOptions::new().name(name).nonblocking(ListenerNonblockingMode::Accept).create_sync().unwrap();
        let remote = thread::spawn({
            let address = address.clone();
            move || connect(&address).unwrap()
        });
        let started = Instant::now();
        let control = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("the listener gave up: {e}"),
            }
            assert!(started.elapsed() < Duration::from_secs(2), "the remote half never connected");
            thread::sleep(Duration::from_millis(1));
        };
        remote.join().unwrap();
        // A child that lives on and ends by itself. `timeout` waits for a key and errors when
        // there is no console to take it from; a ping to no host waits without input.
        let mut child = Command::new(if cfg!(windows) { "cmd" } else { "sleep" });
        if cfg!(windows) {
            child.args(["/c", "ping", "-n", "11", "127.0.0.1", ">nul"]);
        } else {
            child.arg("10");
        }
        let child = child.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let (feed, replies) = mpsc::channel();
        (HostProcess { child, control: std::sync::Arc::new(Mutex::new(control)), replies }, feed)
    }

    #[test]
    fn a_host_that_reports_memory_is_counted_and_one_that_died_is_not() {
        let (process, feed) = living_process();
        let mut group = Group::new();
        group.process = Some(process);
        let group = Arc::new(Mutex::new(group));
        lock(&GROUPS).insert((PathBuf::from("the test host"), "memory-test".into()), Arc::downgrade(&group));

        // What the host sent is taken in and counted.
        feed.send(Message::Memory { bytes: 300 }).unwrap();
        assert_eq!(host_memory(), vec![300]);
        // Reading does not consume the report; it stands until the next one.
        assert_eq!(host_memory(), vec![300]);

        // A process that has stopped is not counted, even with its last report in hand.
        {
            let mut locked = lock(&group);
            if let Some(process) = locked.process.as_mut() {
                let _ = process.child.kill();
            }
        }
        // An ended child stays "running" its own last moment: let it be gone. (`alive` reaps
        // what has gone, so the loop is what reads it.)
        let deadline = Instant::now() + Duration::from_secs(2);
        while {
            let mut locked = lock(&group);
            locked.alive()
        } {
            assert!(Instant::now() < deadline, "the ended host still looks alive");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(host_memory().is_empty(), "a dead host's report must not be counted");
        assert_eq!(lock(&group).ram, None, "the dead host's report is let go of");
    }
}
