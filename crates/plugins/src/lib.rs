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

pub mod library;
pub mod matching;
pub mod offline;
pub mod protocol;
pub mod scan;
pub mod scan_pool;
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
pub use suhina_ipc::{Event, EventKind, MAX_EVENTS, Outcome};
use suhina_ipc::{IpcError, Link};

use protocol::{MAX_LINE, Message, VERSION};
pub use protocol::{ParamInfo, Placement};

/// Blocks in a row without an answer after which a running host counts as hung.
pub const HUNG_AFTER: u32 = 32;

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
        if !sent {
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
        if !sent {
            self.note(Outcome::Busy);
        }
        sent
    }

    /// Counts a joint block this instance followed in, as another instance's link said it went:
    /// a block its host did not answer counts against this instance too, as it may be the one
    /// that is stuck. Real-time safe.
    pub fn followed(&self, outcome: Outcome) {
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
    control: Stream,
    replies: Receiver<Message>,
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
        control.set_nonblocking(false)?;
        let replies = read_lines(control.try_clone()?);
        Ok(Self { child, control, replies })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, message: &Message) -> Result<(), PluginError> {
        let mut line = message.encode();
        line.push('\n');
        self.control.write_all(line.as_bytes())?;
        self.control.flush()?;
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

    /// The latency the host said `instance`'s plug-in now has, if any (and forgets it).
    fn take_latency(&mut self, instance: u32) -> Option<u32> {
        let at = self.latencies.iter().position(|(i, _)| *i == instance)?;
        Some(self.latencies.swap_remove(at).1)
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
    fn load(&mut self, launch: &Launch, instance: u32) -> Result<(u64, u32, Link), PluginError> {
        let link = Link::create(launch.channels, launch.max_frames)?;
        if !self.alive() {
            self.spawn(launch)?;
        }
        let load = Message::Load {
            instance,
            plugin: launch.plugin.clone(),
            link: link.address(),
            sample_rate: launch.sample_rate,
        };
        let loaded = self.request(
            Some(instance),
            &load,
            "load",
            launch.timeout,
            |m| matches!(m, Message::Loaded { instance: i, .. } if *i == instance),
        )?;
        let latency = match loaded {
            Message::Loaded { latency, .. } => latency,
            _ => 0,
        };
        Ok((self.generation, latency, link))
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
    launch: Launch,
    group: Arc<Mutex<Group>>,
    instance: u32,
    /// The group's generation this instance was loaded into.
    generation: u64,
    unanswered: Arc<AtomicU32>,
    latency: u32,
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
        let (generation, latency, link) = lock(&group).load(&launch, instance)?;
        let unanswered = Arc::new(AtomicU32::new(0));
        let plugin = Self {
            launch,
            group,
            instance,
            generation,
            unanswered: unanswered.clone(),
            latency,
            last_state: None,
            params_since: Vec::new(),
            editor_open: false,
            editor_size: None,
            last_health: Health::Running,
        };
        Ok((plugin, PluginAudio { link, unanswered, instance, process: generation }))
    }

    /// Delay the plug-in adds, in frames, as it last reported: when it loaded, or since (see
    /// [`Plugin::follow_latency`]).
    pub fn latency(&self) -> u32 {
        self.latency
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
        let mut group = lock(&self.group);
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let reply = group.request(Some(self.instance), message, what, self.launch.timeout, wanted);
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
        let instance = self.instance;
        let reply = self.request(
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
        self.open_editor_in(owner, false)
    }

    /// [`Plugin::open_editor`] as a child inside the engine's window `owner` (a Win32 `HWND`,
    /// an X11 window id), hidden until [`Plugin::place_editor`] puts it somewhere in it.
    pub fn open_editor_embedded(&mut self, owner: u64) -> Result<(u32, u32), PluginError> {
        self.open_editor_in(Some(owner), true)
    }

    fn open_editor_in(&mut self, owner: Option<u64>, embedded: bool) -> Result<(u32, u32), PluginError> {
        let instance = self.instance;
        let reply = self.request(
            &Message::OpenEditor { instance, owner, embedded },
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
    pub fn place_editor(&mut self, placement: Placement) -> Result<bool, PluginError> {
        let Some(mut group) = try_lock(&self.group) else { return Ok(false) };
        if group.generation != self.generation {
            return Err(PluginError::Protocol("the host process was replaced; restart the plug-in".into()));
        }
        let process = group.process.as_mut().ok_or_else(|| PluginError::Protocol("the host is not running".into()))?;
        process.send(&Message::PlaceEditor { instance: self.instance, placement })?;
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
            let _ = locked.request(
                Some(instance),
                &unload,
                "unload",
                self.launch.timeout,
                |m| matches!(m, Message::Unloaded { instance: i } if *i == instance),
            );
            locked.take_closed(instance);
            locked.take_latency(instance);
        }
        let instance = next_instance();
        let (generation, latency, link) = locked.load(&self.launch, instance)?;
        drop(locked);
        self.instance = instance;
        self.generation = generation;
        self.latency = latency;
        self.editor_open = false;
        self.editor_size = None;
        self.unanswered = Arc::new(AtomicU32::new(0));
        // What `health` says while the group is busy (#188): the new instance, not the old one's
        // `Crashed` or `Hung`.
        self.last_health = Health::Running;
        let audio = PluginAudio { link, unanswered: self.unanswered.clone(), instance, process: generation };
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
            self.launch.timeout,
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
                    if let Some(message) = Message::decode(line.trim_end())
                        && sender.send(message).is_err()
                    {
                        return;
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
        (HostProcess { child, control, replies }, feed)
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
