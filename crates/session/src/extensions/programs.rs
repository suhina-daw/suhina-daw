// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface programs' supervisor (`docs/spec/extensions.md` › Running one,
//! `docs/spec/surface-programs.md`): Suhina's side of the protocol, one control thread per
//! program. The thread starts the shipped runner beside this program with the extension's
//! script, speaks the handshake, sends pings at the plug-in host's health rhythm, restarts a
//! crashed or hung process, and hands the script's send-backs to the session as events — the
//! MIDI router writes them to the program's output ports.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use interprocess::TryClone;
use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Name, NameType, Stream};
use serde_json::Value;
use suhina_plugins::surface::{self, Message};

use crate::CommandError;

/// The protocol this supervisor speaks; a manifest asking for a newer one is refused.
pub const PROTOCOL: u64 = surface::VERSION;
/// Longest one port name pattern a manifest claims.
pub const MAX_PORT_NAME: usize = surface::MAX_NAME;
/// Most live subscriptions a program may keep; the runner refuses a subscribe past it.
pub use suhina_plugins::surface::MAX_SUBSCRIPTIONS;
/// Pings go at the plug-in host's health rhythm; five seconds without an answer is a hang.
const HEALTH_EVERY: Duration = Duration::from_millis(250);
const HUNG_AFTER: Duration = Duration::from_secs(5);
/// A start is done when the connection and the handshake are through.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// Most send-backs kept for the session; past it they are counted, not kept.
const SEND_BACKS_MAX: usize = 4096;
/// A thread's breath between nonblocking reads and polls.
const POLL: Duration = Duration::from_millis(5);
/// How long a stop waits for the runner to answer its quit before killing it: a wedged
/// runner must not hold the session's thread, which joins this one on stop.
const STOP_WAIT: Duration = Duration::from_secs(1);
/// How long a send to the runner may take before the process is ended: the socket is
/// nonblocking for the reads' sake, and its buffer is small (Windows named pipes default
/// to 512 bytes), so a line goes out in the drabs the runner's reads make room for. A
/// runner that stops reading for this long is wedged, and no wait of its script's may
/// outlive the answer it waits on.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often a program may die before the restarts stop and `extensions.restart` is the way.
const CRASHES_ALLOWED: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_secs(60);

/// What the session does to a running program.
pub(crate) enum ToProgram {
    /// The claimed ports changed; the whole list is sent again.
    Ports { inputs: Vec<String>, outputs: Vec<String> },
    /// A raw message read from one of the program's claimed input ports, with the driver's
    /// arrival timestamp in microseconds.
    PortMessage { port: String, timestamp: u64, bytes: Vec<u8> },
    /// End the process, as a crash does (the tests use it first); the supervisor kills only
    /// the process it started, the one named by `pid`.
    TestKill { pid: u32 },
    /// The host's heartbeat on the tests' cue; production sends it on the health rhythm in
    /// the serve loop.
    TestTick,
    /// The user (or the session going away) stops the program.
    Stop,
    /// The session's answer to the script's `command`, named by the command it answered.
    CommandAnswer { token: u64, id: String, ok: bool, error: Option<String> },
    /// The session's answer to the script's `read`, named by the command it answered.
    ReadAnswer { token: u64, id: String, ok: bool, value: Option<Value>, error: Option<String> },
    /// A subscription's value, as it is now, for the command the subscription watches.
    StateUpdate { token: u64, id: String, value: Value },
}

/// What a program reports to the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The handshake is through and the script loaded: the program is running.
    Running,
    /// The process died or hung past the deadline, and the restarts gave up on it.
    Crashed(String),
    /// The script stopped at its error boundary; the process stays up.
    ScriptError(String),
    /// Raw bytes the script asked to send on one of its output ports.
    SendBack { port: String, bytes: Vec<u8> },
    /// Send-backs dropped because the session had not taken the last ones.
    Dropped(usize),
    /// The script asked to run a command, undo included: the same command the window, CLI and
    /// MCP dispatch. The session answers through [`ToProgram::CommandAnswer`].
    Command { token: u64, id: String, params: Value, gesture: Option<String> },
    /// The script asked for a reading command's value. The session answers through
    /// [`ToProgram::ReadAnswer`].
    Read { token: u64, id: String, params: Value },
    /// The script asked to watch a reading command: the store registers it, re-reads it each
    /// look, and pushes a `state` when the value changed.
    Subscribe { token: u64, id: String, params: Value },
    /// The script stopped watching; the store takes the subscription out.
    Unsubscribe { token: u64 },
}

/// The program's event queue and the signal it is reported on: the supervisor pushes and
/// notifies, the session drains, and a waiter blocks on the signal rather than a clock.
/// `terminal` is why the supervisor's thread is over; while it is unset, more events can
/// still come, and a wait for one may answer. Once it is set, no event can come: a wait
/// for one that is not there fails with the reason instead of waiting on nothing.
pub(crate) struct Queue {
    pub(crate) queue: Mutex<VecDeque<Event>>,
    pub(crate) signal: Condvar,
    pub(crate) terminal: Mutex<Option<String>>,
}

impl Default for Queue {
    fn default() -> Self {
        Self { queue: Mutex::new(VecDeque::new()), signal: Condvar::new(), terminal: Mutex::new(None) }
    }
}

/// The program's shared queue, as the handle and the store hold it.
pub(crate) type Shared = Arc<Queue>;

/// A running program's handle: the session's half. Dropping it stops the program. Each start
/// carries a token of its own, so the MIDI router tells a restart's handle from the one it
/// opened the ports for.
pub struct Handle {
    to: Sender<ToProgram>,
    events: Shared,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The runner process's id, for the tests that kill it (a crash is otherwise hard to make
    /// on cue); `None` until the handshake is through.
    pid: Arc<std::sync::atomic::AtomicU32>,
    /// This handle's token: a new one at every [`start`], different for every restart.
    token: u64,
}

/// The next handle's token.
fn next_token() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Handle {
    /// What the program reported since the last drain.
    pub fn drain(&mut self) -> Vec<Event> {
        let mut queue = self.events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.drain(..).collect()
    }

    /// Puts events back at the head of the queue, oldest first: a wait that took one event
    /// leaves the later ones owed ([`crate::extensions::Extensions::wait_for_event`]).
    pub(crate) fn requeue(&self, event: Event) {
        let mut queue = self.events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.push_front(event);
        self.events.signal.notify_one();
    }

    /// The queue and its signal, for the store's blocking drain
    /// ([`crate::extensions::Extensions::wait_for_event`]).
    pub(crate) fn shared(&self) -> Shared {
        Arc::clone(&self.events)
    }

    /// The claimed ports changed; the program hears the whole list again (the MIDI router
    /// calls this where the ports are opened and closed).
    pub fn set_ports(&self, inputs: Vec<String>, outputs: Vec<String>) {
        let _ = self.to.send(ToProgram::Ports { inputs, outputs });
    }

    /// This handle's token: a new one at every restart, so the router's bookkeeping keyed by
    /// it notices when the handle it opened the ports for is no longer the one running.
    pub(crate) fn token(&self) -> u64 {
        self.token
    }

    /// The runner process's id, once the supervisor has spawned it. A test reads this after
    /// the program is running, so the spawn has happened and the id is set.
    pub fn pid(&self) -> Option<u32> {
        let pid = self.pid.load(std::sync::atomic::Ordering::Acquire);
        (pid != 0).then_some(pid)
    }

    /// The order channel, for the router's sink: a raw port message goes through it.
    pub(crate) fn sender(&self) -> Sender<ToProgram> {
        self.to.clone()
    }

    /// Delivers a raw message read from the claimed input port `port`, with the driver's
    /// arrival timestamp in microseconds.
    pub(crate) fn port_message(&self, port: &str, timestamp: u64, bytes: &[u8]) {
        let _ = self.to.send(ToProgram::PortMessage { port: port.to_string(), timestamp, bytes: bytes.to_vec() });
    }

    /// The session's answer to the script's `command`.
    pub(crate) fn command_answer(&self, token: u64, id: &str, ok: bool, error: Option<String>) {
        let _ = self.to.send(ToProgram::CommandAnswer { token, id: id.to_string(), ok, error });
    }

    /// The session's answer to the script's `read`.
    pub(crate) fn read_answer(&self, token: u64, id: &str, ok: bool, value: Option<Value>, error: Option<String>) {
        let _ = self.to.send(ToProgram::ReadAnswer { token, id: id.to_string(), ok, value, error });
    }

    /// A subscription's value, as it is now.
    pub(crate) fn state_update(&self, token: u64, id: &str, value: &Value) {
        let _ = self.to.send(ToProgram::StateUpdate { token, id: id.to_string(), value: value.clone() });
    }

    /// A raw port message with no timestamp, as the tests drive the script without a MIDI
    /// port.
    pub fn test_port_message(&self, port: &str, bytes: &[u8]) {
        self.port_message(port, 0, bytes);
    }

    /// The host's heartbeat, on the tests' cue: production sends it on the health rhythm.
    pub fn test_tick(&self) {
        let _ = self.to.send(ToProgram::TestTick);
    }

    /// A command the tests put straight into the program's event queue — the event the
    /// supervisor would report — so the session's gesture-stream coalescing and its answers
    /// are what they exercise, with no pipeline timing in the way.
    pub fn test_command(&self, token: u64, id: &str, params: &Value, gesture: Option<&str>) {
        let mut queue = self.events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.push_back(Event::Command {
            token,
            id: id.to_string(),
            params: params.clone(),
            gesture: gesture.map(str::to_string),
        });
        self.events.signal.notify_one();
    }

    /// Ends the runner process, as a crash does; the tests make one on cue. Succeeds only when
    /// `pid` is the process this supervisor is running. The kill happens on the supervisor's
    /// thread, which owns the process; the test then waits for the `Crashed` event.
    pub fn test_kill(&self, pid: u32) -> bool {
        if self.pid() != Some(pid) {
            return false;
        }
        self.to.send(ToProgram::TestKill { pid }).is_ok()
    }

    /// Stops the program and waits for its thread to be gone.
    pub fn stop(mut self) {
        let _ = self.to.send(ToProgram::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.to.send(ToProgram::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Where the shipped runner is: beside this program, past a link to it (the plug-in host's
/// way), or up the directory tree — the session's test binaries run from
/// `target/debug/build/<crate>/<hash>/out/`, several levels above `target/debug/` where the
/// runner lives.
pub fn runner_path() -> PathBuf {
    const MAX_LEVELS: usize = 6;
    let name = format!("suhina-lua-runner{}", std::env::consts::EXE_SUFFIX);
    let mut dir = std::env::current_exe()
        .ok()
        .map(|exe| exe.canonicalize().unwrap_or(exe))
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    for _ in 0..=MAX_LEVELS {
        let Some(current) = dir else { break };
        let candidate = current.join(&name);
        // `is_file`, not `exists`: cargo's `build/<crate>/` output directories carry the crate's
        // bare name, and spawning a directory reports Permission denied.
        if candidate.is_file() {
            return candidate;
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    PathBuf::from(&name)
}

/// Starts `extension`'s script (`script`, in its store `folder`) in the shipped runner, on a
/// thread of its own. The handshake is the thread's; the handle hears `Running` when it is
/// through, and everything after. A program starts with no ports open; the MIDI router sends
/// the real ones with [`Handle::set_ports`] when it opens them.
pub fn start(
    id: &str,
    runner: &str,
    folder: &Path,
    script: &str,
    initial_ports: (Vec<String>, Vec<String>),
) -> Result<Handle, CommandError> {
    if runner != "lua" {
        return Err(format!("the runner {runner:?} is not one Suhina ships").into());
    }
    let runner = runner_path();
    if !runner.exists() {
        return Err(format!("the shipped runner {} is not beside this program", runner.display()).into());
    }
    // The script is launched contained: the path the manifest names reaches only what the
    // folder holds, a link inside it not excepted.
    super::contained(folder, Path::new(script))
        .map_err(|reason| crate::CommandError::new(format!("the script {}: {reason}", script)))?;
    let script_path = folder.join(script);
    if !script_path.exists() {
        return Err(format!("the script {} is missing", script_path.display()).into());
    }
    let (to, from) = channel();
    let events: Shared = Arc::new(Queue::default());
    let id = id.to_string();
    let thread_events = Arc::clone(&events);
    let pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let thread_pid = Arc::clone(&pid);
    let token = next_token();
    let thread = std::thread::Builder::new()
        .name("suhina-surface".into())
        .spawn(move || run(&id, &runner, &script_path, from, &thread_events, &thread_pid, initial_ports))
        .map_err(|e| crate::CommandError::new(format!("the program's thread cannot be started: {e}")))?;
    Ok(Handle { to, events, thread: Some(thread), pid, token })
}

/// The thread: start the process, watch it, restart it when it dies or hangs, until stopped.
/// `ports` begins with `initial_ports` — what the session knew the program's device held when
/// the program was started, so its first process hears it at its handshake — and what the
/// session sends later is kept across the restarts: the new process hears it at its
/// handshake, so a crash costs no port message.
fn run(
    id: &str,
    runner: &Path,
    script: &Path,
    from: Receiver<ToProgram>,
    events: &Shared,
    pid: &std::sync::atomic::AtomicU32,
    initial_ports: (Vec<String>, Vec<String>),
) {
    let mut crashes: Vec<Instant> = Vec::new();
    let mut ports: (Vec<String>, Vec<String>) = initial_ports;
    let mut pending = Pending::default();
    let mut first_crash = String::new();
    loop {
        // Too many crashes too close together: the restarts stop and the user decides.
        crashes.retain(|at| at.elapsed() < CRASH_WINDOW);
        if crashes.len() >= CRASHES_ALLOWED {
            let reason = format!("{id} crashed {CRASHES_ALLOWED} times in a minute; restart it by hand");
            let reason =
                if first_crash.is_empty() { reason } else { format!("{reason}; the first one was: {first_crash}") };
            report(events, Event::Crashed(reason.clone()));
            give_up(events, &reason);
            return;
        }
        match watch(runner, script, &from, events, pid, &mut ports, &mut pending) {
            Ok(()) => {
                suhina_plugins::trace!("surface {id} stopped cleanly");
                give_up(events, "the program stopped");
                return;
            }
            Err(reason) => {
                suhina_plugins::trace!("surface {id} crashed: {reason}");
                if first_crash.is_empty() {
                    first_crash = reason.clone();
                }
                crashes.push(Instant::now());
                report(events, Event::Crashed(reason.clone()));
            }
        }
    }
}

/// The runner's process, ended through one path: every error goes through [`Child::end`], so a
/// killed child is never left behind and never waited on twice.
struct Child {
    child: std::process::Child,
}

impl Child {
    /// Ends the process on an error path and says why.
    fn end(&mut self, reason: String) -> String {
        let _ = self.child.kill();
        let status = self.child.wait();
        suhina_plugins::trace!("surface child ended ({reason}): {:?}", status.map(|s| s.to_string()));
        reason
    }

    /// Whether the process is gone.
    fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Waits for the process and says how it ended.
    fn wait(&mut self) -> Result<String, String> {
        self.child.wait().map(|s| s.to_string()).map_err(|_| "it could not be waited for".into())
    }

    /// Ends the process on a stop: the quit was sent; wait a moment for the runner to answer
    /// it, then kill, so a wedged runner holds the caller only for [`STOP_WAIT`].
    fn stop(&mut self) {
        let deadline = Instant::now() + STOP_WAIT;
        while self.try_wait().is_none() && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How many bytes the send-backs waiting for the session may hold, with their count cap: the
/// runner's own send queue's budget, the same policy on the host's side of the pipe.
const SEND_BACKS_BYTES: usize = 8 * 1024 * 1024;

/// One send-back's enqueue, whatever the phase — the startup's and the running loop's alike:
/// the count cap and the bytes cap, and what did not fit counted ([`note_dropped`], as the
/// runner's own queue does). A waiter of the store's blocking drain sleeps on the queue's
/// signal; a push without the notify would leave it there however many send-backs come.
fn enqueue_send_back(events: &Shared, port: String, bytes: Vec<u8>, dropped: &mut usize) {
    let mut queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let count = queue.iter().filter(|e| matches!(e, Event::SendBack { .. })).count();
    let held: usize = queue
        .iter()
        .filter_map(|e| match e {
            Event::SendBack { bytes, .. } => Some(bytes.len()),
            _ => None,
        })
        .sum();
    if count < SEND_BACKS_MAX && held + bytes.len() <= SEND_BACKS_BYTES {
        queue.push_back(Event::SendBack { port, bytes });
        *dropped = 0;
        drop(queue);
    } else {
        drop(queue);
        note_dropped(events, dropped);
    }
    // The queue's signal is for the store's blocking drain: a push without it leaves a waiter
    // hanging however many send-backs come. The window is woken too, not at its next frame:
    // while nothing plays, its meters ask for no frames, and a controller's lights (a tick or
    // a port handler's send-back) would wait for the background repaints. The same wake the
    // plug-in host gives its unasked news (#714).
    events.signal.notify_one();
    suhina_plugins::wake();
}

/// One send-back drop — the queue's caps' or the parked lines' ([`Link::wait_for_room`]) —
/// counted and said the caps' way: the first and every 64th. The count rides the `Dropped`
/// event the store turns into the program's last error. The signal wakes a waiter like
/// [`enqueue_send_back`]'s does: a parked drop is a drop all the same, and on Windows'
/// small pipes it is the one the storm takes.
fn note_dropped(events: &Shared, dropped: &mut usize) {
    *dropped += 1;
    if *dropped == 1 || dropped.is_multiple_of(64) {
        let mut queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.push_back(Event::Dropped(*dropped));
    }
    events.signal.notify_one();
    suhina_plugins::wake();
}

/// The requests the process now running has made, per kind: each token with the command it
/// asked about. The answers and updates the session queued are named by their command, so a
/// dead process's leftover — its tokens begin again with the replacement — cannot complete a
/// fresh wait that happens to reuse the number: only the same command's answer does.
#[derive(Default)]
struct Pending {
    commands: std::collections::BTreeMap<u64, String>,
    reads: std::collections::BTreeMap<u64, String>,
    subs: std::collections::BTreeMap<u64, String>,
}

impl Pending {
    fn clear(&mut self) {
        self.commands.clear();
        self.reads.clear();
        self.subs.clear();
    }
}

/// One order from the session, applied to the program's link and process. A send that
/// cannot complete is returned: the runner stopped reading, and the caller ends the
/// process — a wait of the script's must not outlive the answer it waits on. A Ports
/// order is remembered, so the next process hears the same ports at its handshake. An
/// answer whose token the process now running never asked about is dropped: it is a dead
/// process's leftover, and the tokens begin again with each process.
fn apply(
    order: ToProgram,
    link: &mut Link,
    child: &mut Child,
    ports: &mut (Vec<String>, Vec<String>),
    pending: &mut Pending,
) -> Result<(), String> {
    match order {
        ToProgram::Ports { inputs, outputs } => {
            *ports = (inputs.clone(), outputs.clone());
            link.send(&Message::Ports { inputs, outputs })
        }
        ToProgram::PortMessage { port, timestamp, bytes } => {
            link.send(&Message::PortMessage { port, timestamp, bytes })
        }
        ToProgram::TestKill { pid } => {
            // The id check: between the order and this line the process may have died and been
            // replaced, and the kill must reach the one it was asked for.
            if child.child.id() == pid {
                let _ = child.child.kill();
            }
            Ok(())
        }
        ToProgram::TestTick => link.send(&Message::Tick),
        ToProgram::Stop => {
            suhina_plugins::trace!("surface stop order: quit sent");
            link.send(&Message::Quit)?;
            child.stop();
            Ok(())
        }
        ToProgram::CommandAnswer { token, id, ok, error } => {
            if pending.commands.get(&token) == Some(&id) {
                pending.commands.remove(&token);
                link.send(&Message::CommandResult { token, ok, error })
            } else {
                Ok(())
            }
        }
        ToProgram::ReadAnswer { token, id, ok, value, error } => {
            if pending.reads.get(&token) == Some(&id) {
                pending.reads.remove(&token);
                link.send(&Message::ReadResult { token, ok, value, error })
            } else {
                Ok(())
            }
        }
        ToProgram::StateUpdate { token, id, value } => {
            if pending.subs.get(&token) == Some(&id) {
                link.send(&Message::State { token, value })
            } else {
                Ok(())
            }
        }
    }
}

/// The script's requests, forwarded to the session in the order they came: true for the four
/// the session services (a command, a read, a subscription and its end), so the caller's
/// catch-all does not refuse them. A request's token is noted, so its answers and updates
/// are known to be the process now running's.
fn is_request(message: &Message, events: &Shared, pending: &mut Pending) -> bool {
    match message {
        Message::Command { token, id, params, gesture } => {
            report(
                events,
                Event::Command { token: *token, id: id.clone(), params: params.clone(), gesture: gesture.clone() },
            );
            pending.commands.insert(*token, id.clone());
            true
        }
        Message::Read { token, id, params } => {
            report(events, Event::Read { token: *token, id: id.clone(), params: params.clone() });
            pending.reads.insert(*token, id.clone());
            true
        }
        Message::Subscribe { token, id, params } => {
            report(events, Event::Subscribe { token: *token, id: id.clone(), params: params.clone() });
            pending.subs.insert(*token, id.clone());
            true
        }
        Message::Unsubscribe { token } => {
            report(events, Event::Unsubscribe { token: *token });
            pending.subs.remove(token);
            true
        }
        _ => false,
    }
}

/// One process, from the spawn to its end: the handshake, the pings, the restarts' verdict.
/// `Err` is why the process needs another one; `Ok` says the program was stopped. `ports` is
/// what the session last sent, heard by the new process at its handshake; `pending` begins
/// empty — the tokens begin again with each process, and a dead one's answers are dropped.
fn watch(
    runner: &Path,
    script: &Path,
    from: &Receiver<ToProgram>,
    events: &Shared,
    pid: &std::sync::atomic::AtomicU32,
    ports: &mut (Vec<String>, Vec<String>),
    pending: &mut Pending,
) -> Result<(), String> {
    pending.clear();
    let (address, name) = control_name().map_err(|e| format!("the control socket's name cannot be made: {e}"))?;
    let listener = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()
        .map_err(|e| format!("the control socket cannot be made: {e}"))?;
    // The session's orders that arrive before the process does wait here for the running
    // loop, with the replies answered early in the startup loop below.
    let mut deferred: Vec<ToProgram> = Vec::new();
    // A stop that arrived before there is a process: nothing to end.
    match from.try_recv() {
        Ok(ToProgram::Stop) => {
            suhina_plugins::trace!("surface stop before the process");
            return Ok(());
        }
        Err(TryRecvError::Disconnected) => {
            suhina_plugins::trace!("surface order channel disconnected before the process");
            return Ok(());
        }
        Ok(order) => deferred.push(order),
        Err(TryRecvError::Empty) => {}
    }
    let child = suhina_plugins::host_command(runner)
        .args(["--control", &address, &script.display().to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("the runner cannot be started: {e}"))?;
    pid.store(child.id(), std::sync::atomic::Ordering::Relaxed);
    let mut child = Child { child };
    let deadline = Instant::now() + START_TIMEOUT;
    let stream = loop {
        // A stop while the child is still connecting: there is no link to send a quit on,
        // so the process is ended and reaped — the watch is over the `Ok` way, not a
        // crash — and the caller's join is not held through the connection's timeout.
        match from.try_recv() {
            Ok(ToProgram::Stop) => {
                suhina_plugins::trace!("surface stop while the runner connects");
                child.stop();
                return Ok(());
            }
            Err(TryRecvError::Disconnected) => {
                suhina_plugins::trace!("surface order channel disconnected while the runner connects");
                child.stop();
                return Ok(());
            }
            Ok(order) => deferred.push(order),
            Err(TryRecvError::Empty) => {}
        }
        match listener.accept() {
            Ok(stream) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(child.end(format!("the runner's connection cannot be taken: {e}"))),
        }
        if child.try_wait().is_some() {
            let reason = child.wait().map(|s| s.to_string()).unwrap_or_else(|_| "it exited".into());
            return Err(format!("the runner exited while starting: {reason}"));
        }
        if Instant::now() >= deadline {
            return Err(child.end("the runner did not connect in 30 s".into()));
        }
        std::thread::sleep(POLL);
    };
    stream.set_nonblocking(true).map_err(|e| child.end(format!("the runner's connection cannot be used: {e}")))?;
    let mut link = Link::new(stream).map_err(|reason| child.end(reason))?;

    // The handshake: hello each way, then the ports the session last sent — empty at a fresh
    // start, the running set at a restart — then the script's load. The socket stays
    // nonblocking: the read returns None until the runner speaks, and the loop polls. Orders
    // that arrive while the script loads are kept for the running loop, not dropped, except
    // the session's replies to requests the chunk made while loading: those go out now (the
    // chunk cannot finish without them). A Ports message sent while the script loads is
    // still the program's ports.
    link.send(&Message::Hello { version: PROTOCOL }).map_err(|reason| child.end(reason))?;
    link.send(&Message::Ports { inputs: ports.0.clone(), outputs: ports.1.clone() })
        .map_err(|reason| child.end(reason))?;
    let mut script_errored = false;
    let mut startup_dropped = 0usize;
    loop {
        // A waiting write's lines that did not fit the parked caps are counted the queue's
        // way.
        for _ in 0..link.take_parked_dropped() {
            note_dropped(events, &mut startup_dropped);
        }
        match link.read().map_err(|reason| child.end(reason))? {
            Some(line) => match Message::decode(&line) {
                // A request from the chunk, forwarded to the session: the answer reaches it
                // when the session's next look services it.
                Some(message) if is_request(&message, events, pending) => {}
                Some(Message::Hello { version }) if version == PROTOCOL => {}
                Some(Message::Hello { version }) => {
                    let _ = link.send(&Message::Error {
                        message: format!("version {version} is not spoken; this host speaks {PROTOCOL}"),
                    });
                    return Err(child.end(format!("the runner speaks version {version}, not {PROTOCOL}")));
                }
                Some(Message::Ready) => break,
                Some(Message::ScriptError { handler, message }) => {
                    // The script stopped at its boundary: the process stays up, no `ready`
                    // comes, and `extensions.restart` is the way back. The watch below keeps
                    // answering the process's health, but nothing else comes from it.
                    report(events, Event::ScriptError(format!("{handler}: {message}")));
                    script_errored = true;
                    break;
                }
                Some(Message::Send { port, bytes }) => {
                    // The chunk runs before `ready`, and its `on_ports` answers the ports the
                    // host sent with a full LED rewrite: the send-backs are queued like the
                    // running loop's, taken when the program is running.
                    enqueue_send_back(events, port, bytes, &mut startup_dropped);
                }
                other => {
                    suhina_plugins::trace!("programs handshake refused {other:?}");
                    let _ = link.send(&Message::Error { message: "not a message for the host".into() });
                }
            },
            None => {
                // Nothing whole arrived yet in nonblocking mode; the checks below poll.
            }
        }
        match from.try_recv() {
            Ok(ToProgram::Stop) | Err(TryRecvError::Disconnected) => {
                let _ = link.send(&Message::Quit);
                child.stop();
                return Ok(());
            }
            Ok(order) => {
                // A reply the session made to a request the chunk forwarded while it loads —
                // a `suhina.read`'s or `suhina.command`'s answer, the first state of a
                // `suhina.subscribe` — goes out now: the chunk cannot finish and send
                // `Ready` until it hears it, and a reply held for the running loop wedges
                // the startup until its deadline. Unsolicited input (the ports, the port
                // messages) still waits for the ready script.
                if matches!(
                    order,
                    ToProgram::CommandAnswer { .. } | ToProgram::ReadAnswer { .. } | ToProgram::StateUpdate { .. }
                ) {
                    if let Err(reason) = apply(order, &mut link, &mut child, ports, pending) {
                        return Err(child.end(format!("the runner stopped taking the host's messages: {reason}")));
                    }
                } else {
                    deferred.push(order);
                }
            }
            Err(TryRecvError::Empty) => {}
        }
        // The process's own end is what says the load will never finish — a read that
        // answers with nothing is not it (see [`Link::read`]).
        if child.try_wait().is_some() {
            let reason = child.wait().map(|s| s.to_string()).unwrap_or_else(|_| "it exited".into());
            return Err(format!("the runner exited while starting: {reason}"));
        }
        if Instant::now() >= deadline {
            return Err(child.end("the script did not load in 30 s".into()));
        }
        std::thread::sleep(POLL);
    }
    for order in deferred {
        let stopped = matches!(order, ToProgram::Stop);
        if let Err(reason) = apply(order, &mut link, &mut child, ports, pending) {
            return Err(child.end(format!("the runner stopped taking the host's messages: {reason}")));
        }
        if stopped {
            return Ok(());
        }
    }
    // Running only now: the script is loaded and everything sent before it is applied, its
    // ports among them, so what the session does next reaches the script.
    if !script_errored {
        report(events, Event::Running);
    }

    // Running: pings out at the health rhythm, everything the script says taken down. A script
    // stopped at its boundary keeps its process, so this loop keeps watching it too.
    let mut last_heard = Instant::now();
    let mut last_ping = Instant::now();
    let mut ping = 0u64;
    let mut dropped = 0usize;
    loop {
        // A waiting write's lines that did not fit the parked caps are counted the queue's
        // way.
        for _ in 0..link.take_parked_dropped() {
            note_dropped(events, &mut dropped);
        }
        // Everything the session asked for, or the end of it — drained, so a burst of orders
        // (the answers of a fader's commands among them) is not paid one loop breath each.
        loop {
            match from.try_recv() {
                Ok(ToProgram::Stop) => {
                    suhina_plugins::trace!("surface stop received (running loop)");
                    let _ = link.send(&Message::Quit);
                    child.stop();
                    return Ok(());
                }
                Err(TryRecvError::Disconnected) => {
                    suhina_plugins::trace!("surface order channel disconnected (running loop)");
                    let _ = link.send(&Message::Quit);
                    child.stop();
                    return Ok(());
                }
                Ok(order) => {
                    if let Err(reason) = apply(order, &mut link, &mut child, ports, pending) {
                        return Err(child.end(format!("the runner stopped taking the host's messages: {reason}")));
                    }
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        // Everything the script has sent, drained the same way.
        loop {
            match link.read() {
                Ok(Some(line)) => {
                    // Any word from the runner is liveness, not only the ping's own answer: a
                    // pong can sit behind megabytes of send-backs while a loaded machine
                    // drains them, and the runner streaming them is the opposite of a hang.
                    last_heard = Instant::now();
                    match Message::decode(&line) {
                        Some(Message::Pong { .. }) => {}
                        Some(Message::Send { port, bytes }) => {
                            enqueue_send_back(events, port, bytes, &mut dropped);
                        }
                        Some(Message::ScriptError { handler, message }) => {
                            report(events, Event::ScriptError(format!("{handler}: {message}")));
                        }
                        Some(Message::Hello { .. }) | Some(Message::Ready) => {}
                        Some(message) if is_request(&message, events, pending) => {}
                        Some(Message::Error { message }) => {
                            return Err(child.end(format!("the runner sent an error: {message}")));
                        }
                        Some(other) => {
                            suhina_plugins::trace!("programs refused {other:?}");
                            let _ = link.send(&Message::Error { message: "not a message for the host".into() });
                        }
                        None => {
                            let _ = link.send(&Message::Error { message: "not a message the host knows".into() });
                        }
                    }
                }
                Ok(None) => break,
                Err(reason) => return Err(child.end(reason)),
            }
        }
        if child.try_wait().is_some() {
            let reason = child.wait().map(|s| s.to_string()).unwrap_or_else(|_| "it exited".into());
            return Err(child.end(format!("the runner exited: {reason}")));
        }
        if last_heard.elapsed() >= HUNG_AFTER {
            return Err(child.end("the runner said nothing for 5 s".into()));
        }
        if last_ping.elapsed() >= HEALTH_EVERY {
            ping += 1;
            if let Err(reason) = link.send(&Message::Ping { id: ping }) {
                return Err(child.end(format!("the runner stopped taking the host's messages: {reason}")));
            }
            // The script's heartbeat rides the health rhythm: a controller's lights the device
            // drives itself (its press flashes and release darkenings, some releases dropped
            // whole) drift from what the software says, and the script's periodic re-assert
            // heals them. The runner drops a tick freely while a handler is busy; the next is
            // never far.
            if let Err(reason) = link.send(&Message::Tick) {
                return Err(child.end(format!("the runner stopped taking the host's messages: {reason}")));
            }
            last_ping = Instant::now();
        }
        std::thread::sleep(POLL);
    }
}

/// The supervisor is over: a waiter for an event that can no longer come learns why now,
/// not never. The reason stays, so a later wait fails with it at once.
fn give_up(events: &Shared, reason: &str) {
    let mut terminal = events.terminal.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if terminal.is_none() {
        *terminal = Some(reason.to_string());
    }
    events.signal.notify_all();
}

fn report(events: &Shared, event: Event) {
    let mut queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    queue.push_back(event);
    events.signal.notify_one();
    // The window is woken now, not at its next frame: while nothing plays, its meters ask for
    // no frames, and a controller's command (a fader's, on the return trip) would wait for the
    // background repaints — a moment when playing, a visible lag when stopped. The same wake
    // the plug-in host gives its unasked news (#714).
    suhina_plugins::wake();
}

/// A fresh name for a control socket, the plug-in host's scheme.
fn control_name() -> Result<(String, Name<'static>), String> {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id =
        format!("suhina-surface-{}-{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let address = if GenericNamespaced::is_supported() {
        format!("ns:{id}")
    } else {
        format!("fs:{}", std::env::temp_dir().join(id).display())
    };
    suhina_plugins::parse_address(&address).map_err(|e| format!("bad control address {address:?}: {e}"))
}

/// The control socket's halves, with the runner's line discipline: capped lines, and a read
/// that does not wait.
struct Link {
    reader: BufReader<Stream>,
    writer: Stream,
    partial: Vec<u8>,
    /// Lines read while a write waited for room (see [`Link::send`]): the main loop takes
    /// them before the socket, so nothing read to unjam the peer is lost or reordered. They
    /// hold the send-backs' own caps (`parked_max`, `parked_bytes_max`): a peer streaming
    /// while it never reads cannot grow this queue past what the session's own would take
    /// anyway, and a line past the caps goes, counted in [`Link::parked_dropped`].
    parked: VecDeque<String>,
    /// How many bytes `parked` holds.
    parked_bytes: usize,
    parked_max: usize,
    parked_bytes_max: usize,
    /// Lines read to unjam a waiting write that did not fit the parked caps, for the watch
    /// loops to say the caps' way (the first and every 64th, [`note_dropped`]).
    parked_dropped: usize,
    /// How long a write that must wait gives the runner before the wait is over: production's
    /// [`WRITE_TIMEOUT`]; a test may let the dance take the scheduling it gets.
    write_timeout: Duration,
    /// A write that did not complete: the line is on the wire unfinished, and anything
    /// appended to it would be read as part of it. No further writes go out; the caller
    /// ends the process and the restart takes over.
    dead: bool,
}

impl Link {
    fn new(stream: Stream) -> Result<Self, String> {
        let writer = TryClone::try_clone(&stream).map_err(|e| format!("the socket's halves cannot split: {e}"))?;
        Ok(Self {
            // 64 KiB a fill: a one-MiB line the runner streams is assembled in sixteenths,
            // and every fill that finds nothing breathes `POLL` — an 8 KiB reader paid that
            // breath sixteen times over and let the write timeout end healthy runners under
            // load (#1670).
            reader: BufReader::with_capacity(64 * 1024, stream),
            writer,
            partial: Vec::new(),
            parked: VecDeque::new(),
            parked_bytes: 0,
            parked_max: SEND_BACKS_MAX,
            parked_bytes_max: SEND_BACKS_BYTES,
            parked_dropped: 0,
            write_timeout: WRITE_TIMEOUT,
            dead: false,
        })
    }

    /// Sends one message, the whole line or nothing less than the process's end: the
    /// socket is nonblocking for the reads' sake, so a write on a full buffer returns
    /// `WouldBlock` and the line waits in the drabs the runner's reads make room for,
    /// continuing from where it stopped. Discarding such a write silently dropped a
    /// `state` or a port message whole (a controller dark, a test's marker never sent)
    /// wherever the pipe's buffer ran small. While it waits, readable lines are read and
    /// parked ([`Link::wait_for_room`]): a full duplex pair whose buffers both run small
    /// jams — the peer blocked on a write that needs our reads, our write blocked on its
    /// reads — and retrying without reading wedged both for the whole deadline and killed
    /// a healthy runner (#1670; Windows' pipes are a few KiB). Past the write timeout
    /// ([`WRITE_TIMEOUT`] in production) the runner has stopped reading: the error ends the
    /// process, so no wait of the script's outlives the answer it waits on, and the link is
    /// marked dead — a line left unfinished on the wire must not have the next one appended
    /// to it.
    fn send(&mut self, message: &Message) -> Result<(), String> {
        if self.dead {
            return Err("the link is dead: a write did not complete".into());
        }
        let line = format!("{}\n", message.encode());
        let deadline = Instant::now() + self.write_timeout;
        let mut written = 0;
        let started = Instant::now();
        let mut waited = false;
        while written < line.len() {
            match self.writer.write(&line.as_bytes()[written..]) {
                Ok(0) => {
                    // A write that took no bytes is no room yet, not a refusal: Windows'
                    // nonblocking pipes answer a full buffer with a zero-byte success rather
                    // than WouldBlock, and a peer that is gone errors instead. The deadline
                    // is the judge, as it is for WouldBlock.
                    waited = true;
                    match self.wait_for_room(deadline) {
                        Ok(true) => {}
                        Ok(false) => {
                            self.dead = true;
                            return Err("the runner stopped reading: the write did not complete in time".into());
                        }
                        Err(reason) => {
                            self.dead = true;
                            return Err(reason);
                        }
                    }
                }
                Ok(n) => written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    waited = true;
                    match self.wait_for_room(deadline) {
                        Ok(true) => {}
                        Ok(false) => {
                            self.dead = true;
                            return Err("the runner stopped reading: the write did not complete in time".into());
                        }
                        Err(reason) => {
                            self.dead = true;
                            return Err(reason);
                        }
                    }
                }
                Err(e) => {
                    self.dead = true;
                    return Err(format!("cannot write: {e}"));
                }
            }
        }
        let spent = started.elapsed();
        if waited && spent.as_millis() >= 50 {
            // A write that had to wait names itself: it is the picture of a jammed pair,
            // and after its wait the queue's order is what keeps the protocol whole.
            suhina_plugins::trace!("surface write done: waited {} ms for {} bytes", spent.as_millis(), line.len());
        }
        Ok(())
    }

    /// One wait for room to write: a readable line goes to `parked` — that read is what
    /// lets a peer blocked on its own write finish — and with nothing to read the poll
    /// breathes. A line past the parked caps is still read (the peer must be unjammed) and
    /// let go, counted in [`Link::parked_dropped`] for the watch loops to say. `Ok(false)`
    /// when the deadline passed before waiting again: the runner has stopped reading. A read
    /// that fails fails the write too, the way the main loop's own read would.
    fn wait_for_room(&mut self, deadline: Instant) -> Result<bool, String> {
        if Instant::now() >= deadline {
            return Ok(false);
        }
        match self.read_socket() {
            Ok(Some(line)) => {
                if self.parked.len() < self.parked_max && self.parked_bytes + line.len() <= self.parked_bytes_max {
                    self.parked_bytes += line.len();
                    self.parked.push_back(line);
                } else {
                    self.parked_dropped += 1;
                }
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(reason) => return Err(reason),
        }
        Ok(true)
    }

    /// The lines a waiting write let go, taken: the watch loops count them the queue's way
    /// ([`note_dropped`]), so a peer streaming past the parked caps is said, not silent.
    fn take_parked_dropped(&mut self) -> usize {
        std::mem::take(&mut self.parked_dropped)
    }

    /// One complete line already readable, without waiting: `None` when nothing whole is there
    /// yet. Lines parked while a write waited ([`Link::send`]) come first, in their order — they
    /// were read earlier, and the main loop's order is the protocol's. A partial line stays put
    /// until its end arrives — and what is already readable is taken whole in one call, so a big
    /// line (a one-MiB send-back's) is not paid one poll breath per bufferful, which a healthy
    /// runner could not outpace.
    ///
    /// An empty read is "nothing now", never a close: the two cannot be told apart on every
    /// platform. On Windows a named pipe in nonblocking mode — which the host's socket is —
    /// answers a read with no data as `ERROR_NO_DATA`, a "nothing yet" whose error kind is
    /// `BrokenPipe`, and the socket layer answers that with the same zero-byte read a closed
    /// connection gives. What ends the watch is the process's own end (the loops' `try_wait`)
    /// or a write that fails, so silence is never taken for a goodbye.
    fn read(&mut self) -> Result<Option<String>, String> {
        if let Some(line) = self.parked.pop_front() {
            self.parked_bytes -= line.len();
            return Ok(Some(line));
        }
        self.read_socket()
    }

    /// One complete line from the socket itself, parked lines aside: the loop
    /// [`Link::wait_for_room`] drains with — going through [`Link::read`] there would serve a
    /// line it just parked back to itself and never touch the socket, which is exactly the jam
    /// it exists to break.
    fn read_socket(&mut self) -> Result<Option<String>, String> {
        loop {
            match self.reader.fill_buf() {
                Ok(available) => {
                    if available.is_empty() {
                        return Ok(None);
                    }
                    if let Some(end) = available.iter().position(|b| *b == b'\n') {
                        let mut line = std::mem::take(&mut self.partial);
                        line.extend_from_slice(&available[..end]);
                        self.reader.consume(end + 1);
                        if line.len() > surface::MAX_LINE {
                            return Err("a line longer than the protocol allows".into());
                        }
                        return String::from_utf8(line).map(Some).map_err(|_| "a line that is not UTF-8 text".into());
                    }
                    let read = available.len();
                    self.partial.extend_from_slice(available);
                    self.reader.consume(read);
                    if self.partial.len() > surface::MAX_LINE {
                        return Err("a line longer than the protocol allows".into());
                    }
                    // What was there is in `partial`; what is readable next is taken at once,
                    // and when the socket has no more, the poll breathes.
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(format!("cannot read: {e}")),
            }
        }
    }
}

#[cfg(test)]
#[path = "programs_tests.rs"]
mod tests;
