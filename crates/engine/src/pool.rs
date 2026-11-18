// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render workers ([ADR 0038](../../../docs/decisions/0038-rendering-on-several-cores.md)): one
//! pool of threads per process that runs independent chains in parallel with the render thread.
//!
//! **Two pools, a lane per renderer.** The process has one real-time pool for playback and one
//! at normal priority for offline renders and exports ([`PoolKind`]): the system preempts an
//! export's workers whenever a real-time thread needs a core, so an export beside playback
//! takes only the time playback leaves, and playback never waits for an export's chain
//! (ADR 0038 › 1). Each [`RenderPool`] starts its workers once. A renderer takes a
//! [`Lane`] from it when it is built ([`process_lane`] for the process's pool): one pair of
//! lock-free rings (`rtrb`) to and from each worker, made with the pool, so every ring keeps one
//! writer and one reader. A lane whose renderer plays to a device is marked real-time
//! ([`Lane::join_real_time`]); a worker takes the real-time lanes' chains before any other's. A renderer that finds no free lane renders alone.
//!
//! **A chain moves by value.** A chain goes to a worker through its lane and comes back the same
//! way once its devices have run and it has staged what it adds where it plays
//! ([`Chain::stage`]). While it is away its place in the graph holds one of the lane's spare
//! chains, made when the lane was taken, so nothing is allocated or freed on the render thread.
//! The render thread still feeds every chain's input and adds the staged sound in chain order
//! ([`crate::sends::run`]), so a sum never depends on which thread ran a chain or when it
//! finished: a render on any number of threads is the same, bit for bit (ADR 0038 › 2).
//!
//! **Late work is waited for** (ADR 0038 › 3). The render thread spins a moment, then sleeps
//! until a worker wakes it; it never yields its core. A chain that comes back after its block's
//! deadline (when its worker finished it) is counted as a missed block of its track.
//!
//! Only chains without plug-ins go to workers in this phase: plug-ins already run in their own
//! processes, with their blocks out in parallel, and their path stays on the render thread.
//! Workers sleep in `park` and are woken with `unpark` (a futex wake on Linux, an event on
//! Windows; no lock and no allocation).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use suhina_device_api::ProcessContext;
use suhina_model::TrackId;

use crate::chain::Chain;

/// The environment variable that sets how many threads render a block.
pub const RENDER_THREADS_VAR: &str = "SUHINA_RENDER_THREADS";

/// The most threads a block is rendered on, the render thread included: the render thread and
/// at most 8 workers (ADR 0038 › 1).
pub const MAX_RENDER_THREADS: usize = 9;

/// A block runs its chains on workers only when at least this many may go: below it, waking a
/// worker costs more than it saves (ADR 0038 › 1; to be set by measurement).
pub const PARALLEL_MIN: usize = 4;

/// How many renderers can hold a lane of one pool at once: the player and the renders and
/// exports beside it. A renderer that finds none free renders alone (in a parallel test run, the
/// renderers past the sixteenth).
const LANES: usize = 16;

/// How long the render thread sleeps at most while a chain is at a worker and no plug-in is
/// waited for; a worker that returns a chain wakes it sooner.
const RETURN_WAIT: Duration = Duration::from_millis(2);

/// The most chains a worker holds at once from one lane, sent and not yet given back: each
/// needs a spare to stand in its place.
const DEPTH: usize = 4;

/// How many times the render thread looks for a returned chain, with a spin-loop hint between,
/// before it sleeps: some microseconds.
const SPINS: usize = 2_000;

/// The number of threads `value` (the environment variable's) asks for: 1 when it is missing,
/// 0, or not a number, at most [`MAX_RENDER_THREADS`].
pub fn render_threads_from(value: Option<&str>) -> usize {
    value.and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_RENDER_THREADS)
}

/// The number of threads `SUHINA_RENDER_THREADS` asks for (see [`render_threads_from`]).
pub fn render_threads_from_env() -> usize {
    render_threads_from(std::env::var(RENDER_THREADS_VAR).ok().as_deref())
}

/// The number of threads the automatic choice renders on: the threads the system offers
/// (`available_parallelism`, logical cores), the render thread included, at most
/// [`MAX_RENDER_THREADS`], so the workers are those less one, at most 8. ADR 0038 › 1 counts
/// physical performance cores; telling them apart is #1522.
pub fn automatic_render_threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from).clamp(1, MAX_RENDER_THREADS)
}

/// The threads a choice of the setting renders on, the render thread included: 0 is the
/// automatic choice ([`automatic_render_threads`]), any other number that many, at most
/// [`MAX_RENDER_THREADS`]; `SUHINA_RENDER_THREADS`, when set, wins over both (for diagnosis).
pub fn render_threads_for(choice: usize) -> usize {
    if std::env::var_os(RENDER_THREADS_VAR).is_some() {
        return render_threads_from_env();
    }
    if choice == 0 { automatic_render_threads() } else { choice.clamp(1, MAX_RENDER_THREADS) }
}

/// A chain sent to a worker, with what its block needs and the thread to wake when it is done.
struct Job {
    index: usize,
    chain: Chain,
    frames: usize,
    context: ProcessContext,
    reply_to: Thread,
}

/// A chain coming back: its index, the chain, whether it ran to the end of the block and
/// staged its sound (false only when a plug-in took the block, which a worker's chain has none
/// of), and whether the worker finished it after the block's deadline.
struct Reply {
    index: usize,
    chain: Chain,
    completed: bool,
    late: bool,
}

/// What the renderers and every worker share.
struct Shared {
    stop: AtomicBool,
    /// The pool renders for playback: its workers join real-time priority. An offline pool's
    /// never do, so the system preempts them for any real-time thread (ADR 0038 › 1).
    real_time_pool: bool,
    /// The block size and sample rate the workers join real-time priority for; 0 while no lane
    /// plays to a device.
    real_time: AtomicUsize,
    real_time_rate: AtomicUsize,
    /// Which lanes play to a device: their chains go first.
    lane_real_time: [AtomicBool; LANES],
}

/// A worker's end of one lane.
struct WorkerEnd {
    jobs: rtrb::Consumer<Job>,
    replies: rtrb::Producer<Reply>,
}

/// A renderer's end of one lane to one worker.
struct RendererEnd {
    jobs: rtrb::Producer<Job>,
    replies: rtrb::Consumer<Reply>,
    thread: Thread,
}

/// What a pool renders for (ADR 0038 › 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolKind {
    /// Playback to a device: the workers join real-time priority with the render thread.
    RealTime,
    /// Offline renders and exports: the workers stay at normal priority, so the system preempts
    /// them whenever a real-time thread needs a core, and an export beside playback takes only
    /// the time playback leaves.
    Offline,
}

/// A process's render workers (see the module's documentation).
pub struct RenderPool {
    shared: Arc<Shared>,
    joins: Mutex<Vec<JoinHandle<()>>>,
    threads: Vec<Thread>,
    /// The lanes not taken: each the renderer's ends to every worker. Taken off the audio
    /// thread, given back when a renderer is dropped.
    free: Mutex<Vec<(usize, Vec<RendererEnd>)>>,
}

impl RenderPool {
    /// A pool of `workers` threads, `None` for none. A thread the system will not start is left
    /// out. Allocates and starts threads: never on the render thread.
    pub fn new(workers: usize, kind: PoolKind) -> Option<Arc<Self>> {
        let workers = workers.min(MAX_RENDER_THREADS - 1);
        if workers == 0 {
            return None;
        }
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            real_time_pool: kind == PoolKind::RealTime,
            real_time: AtomicUsize::new(0),
            real_time_rate: AtomicUsize::new(0),
            lane_real_time: std::array::from_fn(|_| AtomicBool::new(false)),
        });
        // Every lane's rings to and from every worker, made now so a lane never allocates.
        let mut renderer_ends: Vec<Vec<RendererEnd>> = (0..LANES).map(|_| Vec::with_capacity(workers)).collect();
        let mut joins = Vec::with_capacity(workers);
        let mut threads = Vec::with_capacity(workers);
        for n in 0..workers {
            let mut ends = Vec::with_capacity(LANES);
            let mut mine = Vec::with_capacity(LANES);
            for _ in 0..LANES {
                let (job_tx, jobs) = rtrb::RingBuffer::<Job>::new(DEPTH);
                let (replies, reply_rx) = rtrb::RingBuffer::<Reply>::new(DEPTH);
                ends.push(WorkerEnd { jobs, replies });
                mine.push((job_tx, reply_rx));
            }
            let worker_shared = Arc::clone(&shared);
            let spawned = std::thread::Builder::new()
                .name(match kind {
                    PoolKind::RealTime => format!("suhina-render-worker-{n}"),
                    PoolKind::Offline => format!("suhina-offline-worker-{n}"),
                })
                .spawn(move || work(&worker_shared, ends));
            let Ok(join) = spawned else { break };
            let thread = join.thread().clone();
            for (lane, (jobs, replies)) in mine.into_iter().enumerate() {
                if let Some(lane_ends) = renderer_ends.get_mut(lane) {
                    lane_ends.push(RendererEnd { jobs, replies, thread: thread.clone() });
                }
            }
            threads.push(thread);
            joins.push(join);
        }
        if threads.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            shared,
            joins: Mutex::new(joins),
            threads,
            free: Mutex::new(renderer_ends.into_iter().enumerate().rev().collect()),
        }))
    }

    /// The threads that render a block with a lane of this pool, the render thread included.
    pub fn threads(&self) -> usize {
        self.threads.len() + 1
    }

    /// A lane for a renderer at `sample_rate`, `None` when every lane is taken. Allocates its
    /// spare chains: never on the render thread.
    pub(crate) fn lane(self: &Arc<Self>, sample_rate: u32) -> Option<Lane> {
        let (index, ends) = self.free.lock().unwrap_or_else(PoisonError::into_inner).pop()?;
        let workers = ends
            .into_iter()
            .map(|end| LaneWorker {
                end,
                spares: (0..DEPTH)
                    .map(|_| Chain::passthrough(TrackId(u32::MAX), crate::NO_COLUMN, sample_rate))
                    .collect(),
            })
            .collect();
        Some(Lane { pool: Arc::clone(self), index, workers, sent: 0 })
    }
}

impl Drop for RenderPool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        for thread in &self.threads {
            thread.unpark();
        }
        let joins = std::mem::take(&mut *self.joins.lock().unwrap_or_else(PoisonError::into_inner));
        for join in joins {
            let _ = join.join();
        }
    }
}

/// The process's two pools, made the first time each is asked for, with the threads
/// `SUHINA_RENDER_THREADS` asks for unless [`set_process_render_threads`] chose others: one
/// real-time pool for playback, one at normal priority for offline renders and exports.
struct ProcessPools {
    threads: usize,
    real_time: Option<Option<Arc<RenderPool>>>,
    offline: Option<Option<Arc<RenderPool>>>,
}

static PROCESS_POOLS: Mutex<Option<ProcessPools>> = Mutex::new(None);

fn process_pools() -> std::sync::MutexGuard<'static, Option<ProcessPools>> {
    PROCESS_POOLS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sets how many threads the process's renderers render on from now, the render thread included
/// (1: the render thread alone), for playback and for offline renders alike. Renderers built
/// before keep the pool they have. Starts or stops threads: never on the render thread.
pub fn set_process_render_threads(threads: usize) {
    let threads = threads.clamp(1, MAX_RENDER_THREADS);
    let mut pools = process_pools();
    if pools.as_ref().is_some_and(|p| p.threads == threads) {
        return;
    }
    *pools = Some(ProcessPools { threads, real_time: None, offline: None });
}

/// The threads the process's renderers render on, the render thread included.
pub fn process_render_threads() -> usize {
    let mut pools = process_pools();
    pools.get_or_insert_with(from_env).threads
}

fn from_env() -> ProcessPools {
    ProcessPools { threads: render_threads_from_env(), real_time: None, offline: None }
}

/// A lane of the process's pool of `kind` for a renderer at `sample_rate`, `None` when the
/// process renders on one thread or every lane is taken. Starts the pool the first time. Never
/// on the render thread.
pub(crate) fn process_lane(sample_rate: u32, kind: PoolKind) -> Option<Lane> {
    let pool = {
        let mut pools = process_pools();
        let pools = pools.get_or_insert_with(from_env);
        let threads = pools.threads;
        let slot = match kind {
            PoolKind::RealTime => &mut pools.real_time,
            PoolKind::Offline => &mut pools.offline,
        };
        slot.get_or_insert_with(|| RenderPool::new(threads - 1, kind)).clone()
    };
    pool?.lane(sample_rate)
}

/// A renderer's lane to one worker, with the spares that stand in for the chains it holds.
struct LaneWorker {
    end: RendererEnd,
    /// [`DEPTH`] spares, one fewer for each chain the worker holds. Never grows past its
    /// capacity.
    spares: Vec<Chain>,
}

impl LaneWorker {
    /// The chains it holds: sent, and not given back yet.
    fn load(&self) -> usize {
        DEPTH.saturating_sub(self.spares.len())
    }
}

/// A renderer's lane of a [`RenderPool`] (see the module's documentation). Given back to the
/// pool when dropped.
pub(crate) struct Lane {
    pool: Arc<RenderPool>,
    index: usize,
    workers: Vec<LaneWorker>,
    /// The chains sent to workers since the lane was taken (tests read it).
    pub(crate) sent: u64,
}

impl Lane {
    /// The threads that render a block, the render thread included.
    pub(crate) fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// What the lane's pool renders for.
    pub(crate) fn kind(&self) -> PoolKind {
        if self.pool.shared.real_time_pool { PoolKind::RealTime } else { PoolKind::Offline }
    }

    /// Marks the lane as playing to a device with blocks of `frames` frames at `sample_rate`:
    /// the workers join real-time priority, as the render thread does
    /// ([`suhina_realtime::RealTime::join`]), each the next time it wakes, and take this lane's
    /// chains before those of lanes that render offline. Never allocates.
    pub(crate) fn join_real_time(&self, frames: usize, sample_rate: u32) {
        let shared = &self.pool.shared;
        if !shared.real_time_pool {
            return;
        }
        if let Some(flag) = shared.lane_real_time.get(self.index) {
            flag.store(true, Ordering::Release);
        }
        shared.real_time_rate.store(sample_rate as usize, Ordering::Release);
        shared.real_time.store(frames.max(1), Ordering::Release);
        for worker in &self.workers {
            worker.end.thread.unpark();
        }
    }

    /// Whether a worker holds a chain of this lane now.
    pub(crate) fn any_away(&self) -> bool {
        self.workers.iter().any(|w| w.load() > 0)
    }

    /// Hands out the queued chains in `chains`: each to whichever of the render thread and the
    /// workers holds the fewest, the render thread taking a tie. A lone chain so stays on the
    /// render thread and wakes nobody, and the rest spread evenly at once, so a worker that
    /// finishes while the render thread runs a chain of its own still has the next one. The
    /// render thread's share stays queued for it to run. True when a chain was sent. Never
    /// allocates.
    pub(crate) fn distribute(&mut self, chains: &mut [Chain], frames: usize, context: &ProcessContext) -> bool {
        let mut mine = 0usize;
        let mut sent = false;
        for i in 0..chains.len() {
            if !chains.get(i).is_some_and(|c| c.turn.queued) {
                continue;
            }
            let least = (0..self.workers.len())
                .filter_map(|n| self.workers.get(n).filter(|w| !w.spares.is_empty()).map(|w| (n, w.load())))
                .min_by_key(|&(_, load)| load);
            match least {
                Some((n, load)) if load < mine && self.send(chains, n, i, frames, context) => sent = true,
                _ => mine += 1,
            }
        }
        sent
    }

    /// Sends chain `i` to worker `n`, its place in `chains` taken by one of the worker's spares
    /// (whose turn says it is away). False when the worker has no spare left. Never allocates.
    fn send(&mut self, chains: &mut [Chain], n: usize, i: usize, frames: usize, context: &ProcessContext) -> bool {
        let Some(worker) = self.workers.get_mut(n) else { return false };
        let Some(slot) = chains.get_mut(i) else { return false };
        let Some(mut spare) = worker.spares.pop() else { return false };
        slot.turn.queued = false;
        spare.turn = slot.turn;
        spare.turn.away = true;
        let chain = std::mem::replace(slot, spare);
        let job = Job { index: i, chain, frames, context: *context, reply_to: std::thread::current() };
        match worker.end.jobs.push(job) {
            Ok(()) => {
                worker.end.thread.unpark();
                self.sent = self.sent.wrapping_add(1);
                true
            }
            // Never full: the ring holds DEPTH jobs, and a worker holds at most DEPTH chains.
            Err(rtrb::PushError::Full(job)) => {
                if let Some(slot) = chains.get_mut(i) {
                    worker.spares.push(std::mem::replace(slot, job.chain));
                    slot.turn.queued = true;
                }
                false
            }
        }
    }

    /// Puts every chain the workers have given back into its place in `chains`, counts a chain
    /// its worker finished after the block's deadline as a missed block of its track (ADR 0038 ›
    /// 3; judged when the worker finished, not when the render thread took it back), and calls
    /// `back` with its index and whether it completed its block. True when one came back. Never
    /// allocates.
    pub(crate) fn receive(&mut self, chains: &mut [Chain], mut back: impl FnMut(&mut [Chain], usize, bool)) -> bool {
        let mut any = false;
        for w in 0..self.workers.len() {
            while let Some(worker) = self.workers.get_mut(w)
                && let Ok(Reply { index, chain, completed, late }) = worker.end.replies.pop()
            {
                if let Some(slot) = chains.get_mut(index) {
                    let mut chain = chain;
                    chain.turn.away = false;
                    if late {
                        chain.missed_blocks = chain.missed_blocks.saturating_add(1);
                    }
                    worker.spares.push(std::mem::replace(slot, chain));
                }
                back(chains, index, completed);
                any = true;
            }
        }
        any
    }

    /// Waits until a worker gives a chain back, or `slice` passes (a plug-in may answer
    /// meanwhile): it spins a moment first, since a chain often comes back sooner than a
    /// sleeping thread wakes, then sleeps until a worker wakes it. It never yields: on Windows
    /// a yield can give the core to a thread of normal priority for a whole quantum. Never
    /// allocates.
    pub(crate) fn wait(&self, slice: Option<Duration>) {
        let back = || self.workers.iter().any(|w| w.load() > 0 && !w.end.replies.is_empty());
        for _ in 0..SPINS {
            if back() {
                return;
            }
            std::hint::spin_loop();
        }
        if !back() {
            std::thread::park_timeout(slice.unwrap_or(RETURN_WAIT));
        }
    }
}

impl Drop for Lane {
    /// Gives the lane's rings back to the pool, for the next renderer. Every chain is home: a
    /// render waits for each it sends before it returns.
    fn drop(&mut self) {
        if let Some(flag) = self.pool.shared.lane_real_time.get(self.index) {
            flag.store(false, Ordering::Release);
        }
        let ends = std::mem::take(&mut self.workers).into_iter().map(|w| w.end).collect();
        self.pool.free.lock().unwrap_or_else(PoisonError::into_inner).push((self.index, ends));
    }
}

/// A worker's loop: takes the next chain, real-time lanes first, runs it over the rest of its
/// block and stages it, gives it back and wakes the render thread that sent it, until the pool
/// is dropped.
fn work(shared: &Shared, mut lanes: Vec<WorkerEnd>) {
    let mut realtime: Option<suhina_realtime::RealTime> = None;
    while !shared.stop.load(Ordering::Acquire) {
        let frames = shared.real_time.load(Ordering::Acquire);
        if shared.real_time_pool && frames > 0 && realtime.is_none() {
            let rate = u32::try_from(shared.real_time_rate.load(Ordering::Acquire)).unwrap_or(48_000);
            realtime = Some(suhina_realtime::RealTime::join(frames, rate));
        }
        let Some((lane, job)) = next_job(shared, &mut lanes) else {
            if !park_unless_stopping(shared) {
                break;
            }
            if let Some(rt) = realtime.as_mut() {
                rt.waited();
            }
            continue;
        };
        let Job { index, mut chain, frames, context, reply_to } = job;
        let started = Instant::now();
        let completed = !chain.advance(frames, &context);
        if completed {
            chain.finish(frames, &context);
        }
        let late = chain.past_deadline(crate::clock::now());
        // Never full: the ring holds DEPTH replies, and the worker holds at most DEPTH chains
        // of a lane.
        if let Some(end) = lanes.get_mut(lane) {
            let _ = end.replies.push(Reply { index, chain, completed, late });
        }
        reply_to.unpark();
        if let Some(rt) = realtime.as_mut() {
            rt.worked(started.elapsed());
        }
    }
}

/// Parks the worker until something wakes it, unless the pool is stopping; false when it is. The
/// stop is looked at again just before parking: the pool's drop sets it before it wakes the
/// workers, and the wake-up can be spent before the worker parks. Joining real time waits on a
/// channel, which parks the thread too, so a drop that came then left the worker asleep for good
/// and the drop waiting on it (a hang on Linux, where the worker waits for rtkit's answer).
fn park_unless_stopping(shared: &Shared) -> bool {
    if shared.stop.load(Ordering::Acquire) {
        return false;
    }
    std::thread::park();
    true
}

/// The next job for a worker: from a lane that plays to a device if one has any, else from any
/// lane, each in order.
fn next_job(shared: &Shared, lanes: &mut [WorkerEnd]) -> Option<(usize, Job)> {
    for real_time in [true, false] {
        for (n, end) in lanes.iter_mut().enumerate() {
            let flagged = shared.lane_real_time.get(n).is_some_and(|f| f.load(Ordering::Acquire));
            if flagged != real_time {
                continue;
            }
            if let Ok(job) = end.jobs.pop() {
                return Some((n, job));
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
