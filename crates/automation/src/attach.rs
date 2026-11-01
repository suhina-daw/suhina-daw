// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The desktop app's side of ADR 0028: agents work on the projects open in the window.
//!
//! [`AppEndpoints`] runs on a tokio runtime of its own. It holds the build's [`Endpoint::App`]
//! (one instance per build: a second start hands its files over and exits) and, while the person
//! lets agents in, serves MCP on [`Endpoint::Mcp`]. A tool call becomes a [`Job`] that the window
//! runs between frames on its own `Projects`, so an agent's edit is shown at once and is an undo
//! step in the tab's history. Every connection waits until the person allows it.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use rmcp::ServiceExt;
use serde_json::{Value, json};
use suhina_session::Projects;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, watch};

use crate::endpoint::{self, Endpoint, Listener, Stream};
use crate::server::SuhinaMcp;

/// Work an agent's tool call advances once per frame; true finishes the job.
pub type Job = Box<dyn FnMut(&mut Projects) -> bool + Send>;

/// Asks the window to draw a frame soon (`egui::Context::request_repaint`).
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Longest "open these files" message a second start may send.
const MAX_OPEN_MESSAGE: u64 = 64 * 1024;
/// Most jobs the window runs in one frame; the rest wait for the next.
const JOBS_PER_FRAME: usize = 64;

/// What the window learns about agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// A client connected and says it is `client`: ask the person whether to let it in, then
    /// call [`AppEndpoints::answer`].
    Ask { connection: u64, client: String },
    /// A connection ended (the client left, the person disconnected or denied it).
    Gone { connection: u64 },
}

/// The person's answer to a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consent {
    Waiting,
    Allowed,
    Denied,
}

/// One connection, as the window sees it.
struct Connection {
    client: String,
    consent: watch::Sender<Consent>,
    end: Arc<Notify>,
}

/// What the runtime's tasks and the window share.
struct Shared {
    connections: Mutex<HashMap<u64, Connection>>,
    events: mpsc::Sender<AppEvent>,
    wake: Wake,
}

impl Shared {
    fn tell(&self, event: AppEvent) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    fn connections(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Connection>> {
        self.connections.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// How an attached [`SuhinaMcp`] reaches the window: its jobs queue, and the person's answer for
/// its connection.
#[derive(Clone)]
pub struct AppHost {
    jobs: mpsc::Sender<Job>,
    wake: Wake,
    consent: watch::Receiver<Consent>,
}

impl AppHost {
    /// Runs `f` on the window's projects once the person has allowed the connection; waits for
    /// their answer first.
    pub(crate) async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Projects) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let mut f = Some(f);
        self.poll(move |projects, cancelled| {
            if cancelled {
                return Some(Err("the request was cancelled".into()));
            }
            f.take().map(|f| f(projects))
        })
        .await
    }

    /// Advances bounded preparation on frames, without retaining access to the projects.
    /// Cancellation is delivered once so the operation can release its pending state reads.
    pub(crate) async fn poll<T: Send + 'static>(
        &self,
        mut f: impl FnMut(&mut Projects, bool) -> Option<Result<T, String>> + Send + 'static,
    ) -> Result<T, String> {
        let mut consent = self.consent.clone();
        loop {
            let now = *consent.borrow_and_update();
            match now {
                Consent::Allowed => break,
                Consent::Denied => return Err("the person did not allow this connection".into()),
                Consent::Waiting => {
                    if consent.changed().await.is_err() {
                        return Err("the connection was closed".into());
                    }
                }
            }
        }
        let (send, receive) = tokio::sync::oneshot::channel();
        let mut send = Some(send);
        let job: Job = Box::new(move |projects| {
            let cancelled = send.as_ref().is_none_or(|s| s.is_closed());
            if let Some(result) = f(projects, cancelled) {
                if let Some(send) = send.take() {
                    let _ = send.send(result);
                }
                true
            } else {
                cancelled
            }
        });
        self.jobs.send(job).map_err(|_| "Suhina has closed".to_string())?;
        (self.wake)();
        receive.await.map_err(|_| "Suhina has closed".to_string())?
    }
}

/// Another app of this build is running; [`Running::hand_over`] gives it the files to open.
#[derive(Debug, Clone)]
pub struct Running {
    key: String,
}

impl Running {
    /// Asks the running app to open `files` as tabs (none: only come forward) and waits for it
    /// to say it got them.
    pub fn hand_over(&self, files: &[PathBuf]) -> io::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let files: Vec<String> = files
            .iter()
            .map(|f| f.canonicalize().unwrap_or_else(|_| f.clone()))
            .map(|f| crate::workspace::display_path(&f))
            .collect();
        runtime.block_on(async {
            let mut stream = endpoint::connect(&self.key, Endpoint::App).await?;
            let mut line = json!({ "open": files }).to_string();
            line.push('\n');
            stream.write_all(line.as_bytes()).await?;
            stream.flush().await?;
            let mut answer = String::new();
            let mut reader = BufReader::new(&mut stream).take(1024);
            tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut answer))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the running Suhina did not answer"))??;
            Ok(())
        })
    }
}

/// `suhina-cli mcp --attach`: copies stdin to the MCP endpoint of the app of the build in
/// `folder` and its answers to stdout, until either side ends. Says which is missing when the
/// app is not running or does not let agents in.
pub async fn attach_stdio(folder: &std::path::Path) -> Result<(), String> {
    let key = endpoint::build_key(folder);
    let stream = match endpoint::connect(&key, Endpoint::Mcp).await {
        Ok(stream) => stream,
        Err(e) if endpoint::is_absent(&e) => {
            return Err(match endpoint::connect(&key, Endpoint::App).await {
                Ok(_) => "Suhina is running, but it does not let agents in: turn on Settings › Agents and updates › Agents › \
                          AI agents edit projects"
                    .into(),
                Err(_) => format!(
                    "Suhina is not running: start the suhina app in {} (an agent reaches the app of its own build)",
                    crate::workspace::display_path(folder)
                ),
            });
        }
        Err(e) => return Err(format!("could not reach Suhina: {e}")),
    };
    let (mut from_app, mut to_app) = tokio::io::split(stream);
    let requests = async {
        let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut to_app).await;
        let _ = to_app.shutdown().await;
    };
    let answers = async {
        let _ = tokio::io::copy(&mut from_app, &mut tokio::io::stdout()).await;
    };
    // The client closing stdin, or the app ending the connection, ends the bridge.
    tokio::select! {
        () = requests => {}
        () = answers => {}
    }
    Ok(())
}

/// The app's endpoints, owned by the window.
pub struct AppEndpoints {
    runtime: tokio::runtime::Runtime,
    shared: Arc<Shared>,
    jobs: mpsc::Receiver<Job>,
    job_sender: mpsc::Sender<Job>,
    pending: std::cell::RefCell<Vec<Job>>,
    events: mpsc::Receiver<AppEvent>,
    opens: mpsc::Receiver<Vec<PathBuf>>,
    enabled: watch::Sender<bool>,
}

impl AppEndpoints {
    /// Takes the endpoint of the build in `folder`: `Err(Running)` when another app of the build
    /// holds it. `wake` asks the window for a frame when there is something for it.
    pub fn start(folder: &std::path::Path, wake: Wake) -> Result<Self, StartError> {
        let key = endpoint::build_key(folder);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("suhina-agents")
            .enable_all()
            .build()
            .map_err(StartError::Io)?;
        let lock = {
            let _entered = runtime.enter();
            Listener::bind(&key, Endpoint::App)
        };
        let lock = match lock {
            Ok(lock) => lock,
            Err(e) if endpoint::is_taken(&e) => return Err(StartError::Running(Running { key })),
            Err(e) => return Err(StartError::Io(e)),
        };
        let (events_tx, events) = mpsc::channel();
        let (opens_tx, opens) = mpsc::channel();
        let (job_sender, jobs) = mpsc::channel();
        let (enabled, enabled_rx) = watch::channel(false);
        let shared = Arc::new(Shared { connections: Mutex::new(HashMap::new()), events: events_tx, wake });
        runtime.spawn(serve_app(lock, opens_tx, Arc::clone(&shared)));
        runtime.spawn(serve_agents(key, enabled_rx, job_sender.clone(), Arc::clone(&shared)));
        Ok(Self { runtime, shared, jobs, job_sender, pending: Default::default(), events, opens, enabled })
    }

    /// Lets agents connect, or stops listening and ends every connection.
    pub fn set_enabled(&self, on: bool) {
        if *self.enabled.borrow() != on {
            let _ = self.enabled.send(on);
            if !on {
                for connection in self.shared.connections().values() {
                    connection.end.notify_one();
                }
            }
        }
    }

    /// Runs the jobs agents have sent, in order, on the window's projects. Returns how many ran.
    pub fn run_jobs(&self, projects: &mut Projects) -> usize {
        let mut pending = self.pending.borrow_mut();
        let mut ran = pending.len();
        pending.retain_mut(|job| !job(projects));
        while ran < JOBS_PER_FRAME {
            let Ok(mut job) = self.jobs.try_recv() else { break };
            if !job(projects) {
                pending.push(job);
            }
            ran += 1;
        }
        if ran == JOBS_PER_FRAME || !pending.is_empty() {
            (self.shared.wake)();
        }
        ran
    }

    /// What happened with agents since the last call.
    pub fn take_events(&self) -> Vec<AppEvent> {
        self.events.try_iter().collect()
    }

    /// What later starts of this build asked since the last call: `Some` when one did, with the
    /// files to open (none when it had no project; the window only comes forward).
    pub fn take_opens(&self) -> Option<Vec<PathBuf>> {
        let asked: Vec<Vec<PathBuf>> = self.opens.try_iter().collect();
        (!asked.is_empty()).then(|| asked.into_iter().flatten().collect())
    }

    /// The person's answer to [`AppEvent::Ask`]. Deny ends the connection.
    pub fn answer(&self, connection: u64, allow: bool) {
        let connections = self.shared.connections();
        if let Some(c) = connections.get(&connection) {
            let _ = c.consent.send(if allow { Consent::Allowed } else { Consent::Denied });
            if !allow {
                c.end.notify_one();
            }
        }
    }

    /// Ends a connection.
    pub fn disconnect(&self, connection: u64) {
        if let Some(c) = self.shared.connections().get(&connection) {
            c.end.notify_one();
        }
    }

    /// The connections the person has allowed: id and the name the client gives.
    pub fn connected(&self) -> Vec<(u64, String)> {
        let mut list: Vec<(u64, String)> = self
            .shared
            .connections()
            .iter()
            .filter(|(_, c)| *c.consent.borrow() == Consent::Allowed)
            .map(|(id, c)| (*id, c.client.clone()))
            .collect();
        list.sort();
        list
    }

    /// A host for a server in this process, allowed from the start (tests).
    #[doc(hidden)]
    pub fn local_host(&self) -> AppHost {
        let (_, consent) = watch::channel(Consent::Allowed);
        AppHost { jobs: self.job_sender.clone(), wake: Arc::clone(&self.shared.wake), consent }
    }

    /// The runtime the endpoints run on (tests drive a client on it).
    #[doc(hidden)]
    pub fn runtime(&self) -> &tokio::runtime::Runtime {
        &self.runtime
    }
}

/// Why [`AppEndpoints::start`] did not start.
#[derive(Debug)]
pub enum StartError {
    /// Another app of this build is running.
    Running(Running),
    Io(io::Error),
}

/// Serves "open these files" from later starts of the build, for as long as the app runs.
async fn serve_app(mut lock: Listener, opens: mpsc::Sender<Vec<PathBuf>>, shared: Arc<Shared>) {
    loop {
        let Ok(stream) = lock.accept().await else {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        };
        let opens = opens.clone();
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            let files = read_open(stream).await;
            if let Some(files) = files {
                let _ = opens.send(files);
                (shared.wake)();
            }
        });
    }
}

/// Reads one `{"open": [...]}` line and answers it; `None` when it is not one.
async fn read_open(mut stream: Box<dyn Stream>) -> Option<Vec<PathBuf>> {
    let mut line = String::new();
    let mut reader = BufReader::new(&mut stream).take(MAX_OPEN_MESSAGE);
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await.ok()?.ok()?;
    drop(reader);
    let message: Value = serde_json::from_str(line.trim()).ok()?;
    let files = message.get("open")?.as_array()?.iter().filter_map(Value::as_str).map(PathBuf::from).collect();
    let _ = stream.write_all(b"{\"ok\":true}\n").await;
    let _ = stream.flush().await;
    Some(files)
}

/// Listens for agents while `enabled` is on.
async fn serve_agents(key: String, mut enabled: watch::Receiver<bool>, jobs: mpsc::Sender<Job>, shared: Arc<Shared>) {
    let next_id = Arc::new(AtomicU64::new(1));
    loop {
        if !*enabled.borrow_and_update() {
            if enabled.changed().await.is_err() {
                return;
            }
            continue;
        }
        let mut listener = match Listener::bind(&key, Endpoint::Mcp) {
            Ok(listener) => listener,
            Err(_) => {
                // Another process holds it (an app started with --new-instance does not
                // listen, so this is a leftover); try again in a moment.
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_secs(2)) => {}
                    changed = enabled.changed() => if changed.is_err() { return },
                }
                continue;
            }
        };
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    if let Ok(stream) = accepted {
                        let id = next_id.fetch_add(1, Ordering::Relaxed);
                        tokio::spawn(serve_agent(id, stream, jobs.clone(), Arc::clone(&shared)));
                    }
                }
                changed = enabled.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if !*enabled.borrow() {
                        break;
                    }
                }
            }
        }
    }
}

/// One agent's connection: MCP over `stream`, its tool calls waiting for the person's answer.
async fn serve_agent(id: u64, stream: Box<dyn Stream>, jobs: mpsc::Sender<Job>, shared: Arc<Shared>) {
    let (consent_tx, consent) = watch::channel(Consent::Waiting);
    let end = Arc::new(Notify::new());
    let host = AppHost { jobs, wake: Arc::clone(&shared.wake), consent };
    let server = SuhinaMcp::attached(host);
    let running = tokio::select! {
        running = server.serve(tokio::io::split(stream)) => match running {
            Ok(running) => running,
            Err(_) => return,
        },
        () = end.notified() => return,
    };
    let client = running
        .peer()
        .peer_info()
        .map(|info| info.client_info.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "An unnamed agent".to_string());
    let client: String = client.chars().filter(|c| !c.is_control()).take(80).collect();
    shared.connections().insert(id, Connection { client: client.clone(), consent: consent_tx, end: Arc::clone(&end) });
    shared.tell(AppEvent::Ask { connection: id, client });
    tokio::select! {
        _ = running.waiting() => {}
        // Dropping the running service ends the connection.
        () = end.notified() => {}
    }
    shared.connections().remove(&id);
    shared.tell(AppEvent::Gone { connection: id });
}
