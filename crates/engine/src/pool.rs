// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render workers: a small pool of threads that run independent tracks' chains in parallel with
//! the render thread (prototype). `SUHINA_RENDER_THREADS` (or [`crate::Renderer::set_render_threads`])
//! says how many threads render a block: 0 or 1 is the render thread alone, as before; `n` adds
//! `n - 1` workers.
//!
//! A chain goes to a worker by value, through a lock-free ring (`rtrb`) built with the pool, and
//! comes back the same way once its devices have run and it has staged what it adds where it
//! plays ([`Chain::stage`]). While it is away its place in the graph holds a spare chain made
//! with the pool, so nothing is allocated or freed on the render thread. The render thread
//! still feeds every chain's input and adds the staged sound in chain order
//! ([`crate::sends::run`]), so a sum never depends on which thread ran a chain or when it
//! finished: a render with workers is the same, bit for bit, as one without.
//!
//! Only chains without plug-ins go to workers: plug-ins already run in their own processes, with
//! their blocks out in parallel, and their path stays on the render thread as it is.
//!
//! Workers sleep in `park` and are woken with `unpark` (a futex wake on Linux, no lock and no
//! allocation); the render thread waits for a chain's return the same way.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use suhina_device_api::ProcessContext;
use suhina_model::TrackId;

use crate::chain::Chain;

/// The environment variable that sets how many threads render a block.
pub const RENDER_THREADS_VAR: &str = "SUHINA_RENDER_THREADS";

/// The most threads a block is rendered on, the render thread included.
pub const MAX_RENDER_THREADS: usize = 16;

/// How long the render thread sleeps at most while a chain is at a worker and no plug-in is
/// waited for; a worker that returns a chain wakes it sooner.
const RETURN_WAIT: Duration = Duration::from_millis(2);

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

/// A chain sent to a worker, with what its block needs and the thread to wake when it is done.
struct Job {
    index: usize,
    chain: Chain,
    frames: usize,
    context: ProcessContext,
    reply_to: Thread,
}

/// A chain coming back: its index, the chain, and whether it ran to the end of the block and
/// staged its sound (false only when a plug-in took the block, which a worker's chain has none of).
struct Reply {
    index: usize,
    chain: Chain,
    completed: bool,
}

/// What the render thread and every worker share.
struct Shared {
    stop: AtomicBool,
    /// The block size the workers join real-time priority for; 0 while they should not.
    real_time: AtomicUsize,
    sample_rate: u32,
}

/// The render thread's end of one worker.
struct Worker {
    jobs: rtrb::Producer<Job>,
    replies: rtrb::Consumer<Reply>,
    thread: Thread,
    /// What stands in the graph for the chain while it is away.
    spare: Option<Chain>,
    /// The chain it runs, while it runs one.
    busy: Option<usize>,
}

/// The render workers (see the module's documentation).
pub(crate) struct Pool {
    workers: Vec<Worker>,
    joins: Vec<JoinHandle<()>>,
    shared: Arc<Shared>,
    /// The chains sent to workers since the pool was made (tests read it).
    pub(crate) sent: u64,
}

impl Pool {
    /// A pool of `workers` threads for a renderer at `sample_rate`, `None` for none. A thread
    /// the system will not start is left out. Allocates and starts threads: never on the render
    /// thread.
    pub(crate) fn new(workers: usize, sample_rate: u32) -> Option<Self> {
        if workers == 0 {
            return None;
        }
        let shared = Arc::new(Shared { stop: AtomicBool::new(false), real_time: AtomicUsize::new(0), sample_rate });
        let mut pool =
            Self { workers: Vec::with_capacity(workers), joins: Vec::with_capacity(workers), shared, sent: 0 };
        for n in 0..workers {
            let (jobs, job_rx) = rtrb::RingBuffer::<Job>::new(1);
            let (reply_tx, replies) = rtrb::RingBuffer::<Reply>::new(1);
            let shared = Arc::clone(&pool.shared);
            let spawned = std::thread::Builder::new()
                .name(format!("suhina-render-worker-{n}"))
                .spawn(move || work(&shared, job_rx, reply_tx));
            let Ok(join) = spawned else { break };
            pool.workers.push(Worker {
                jobs,
                replies,
                thread: join.thread().clone(),
                spare: Some(Chain::passthrough(TrackId(u32::MAX), crate::NO_COLUMN, sample_rate)),
                busy: None,
            });
            pool.joins.push(join);
        }
        (!pool.workers.is_empty()).then_some(pool)
    }

    /// The threads that render a block, the render thread included.
    pub(crate) fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// Asks the workers to join real-time priority for blocks of `frames` frames, as the render
    /// thread does ([`suhina_realtime::RealTime::join`]); each does so the next time it wakes.
    /// Never allocates.
    pub(crate) fn join_real_time(&self, frames: usize) {
        self.shared.real_time.store(frames.max(1), Ordering::Release);
        for worker in &self.workers {
            worker.thread.unpark();
        }
    }

    /// Whether a worker runs a chain now.
    pub(crate) fn any_away(&self) -> bool {
        self.workers.iter().any(|w| w.busy.is_some())
    }

    /// Sends chain `i` to an idle worker, its place in `chains` taken by the worker's spare
    /// (whose turn says it is away). False when every worker is busy. Never allocates.
    pub(crate) fn send(&mut self, chains: &mut [Chain], i: usize, frames: usize, context: &ProcessContext) -> bool {
        let Some(worker) = self.workers.iter_mut().find(|w| w.busy.is_none() && w.spare.is_some()) else {
            return false;
        };
        let (Some(slot), Some(mut spare)) = (chains.get_mut(i), worker.spare.take()) else { return false };
        slot.turn.queued = false;
        spare.turn = slot.turn;
        spare.turn.away = true;
        let chain = std::mem::replace(slot, spare);
        let job = Job { index: i, chain, frames, context: *context, reply_to: std::thread::current() };
        match worker.jobs.push(job) {
            Ok(()) => {
                worker.busy = Some(i);
                worker.thread.unpark();
                self.sent = self.sent.wrapping_add(1);
                true
            }
            // Never full: a worker gets a job only once it has given the last one back.
            Err(rtrb::PushError::Full(job)) => {
                if let Some(slot) = chains.get_mut(i) {
                    worker.spare = Some(std::mem::replace(slot, job.chain));
                    slot.turn.queued = true;
                }
                false
            }
        }
    }

    /// Puts every chain a worker has given back into its place in `chains`, and calls `back`
    /// with its index and whether it completed its block. True when one came back. Never
    /// allocates.
    pub(crate) fn receive(&mut self, chains: &mut [Chain], mut back: impl FnMut(&mut [Chain], usize, bool)) -> bool {
        let mut any = false;
        for w in 0..self.workers.len() {
            let Some(worker) = self.workers.get_mut(w) else { continue };
            let Ok(Reply { index, chain, completed }) = worker.replies.pop() else { continue };
            worker.busy = None;
            if let Some(slot) = chains.get_mut(index) {
                let mut chain = chain;
                chain.turn.away = false;
                worker.spare = Some(std::mem::replace(slot, chain));
            }
            back(chains, index, completed);
            any = true;
        }
        any
    }

    /// Waits until a worker gives a chain back, or `slice` passes (a plug-in may answer
    /// meanwhile): it spins a moment first, since a chain often comes back sooner than a
    /// sleeping thread wakes, then sleeps until a worker wakes it. Never allocates.
    pub(crate) fn wait(&self, slice: Option<Duration>) {
        let back = || self.workers.iter().any(|w| w.busy.is_some() && !w.replies.is_empty());
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

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        for worker in &self.workers {
            worker.thread.unpark();
        }
        for join in self.joins.drain(..) {
            let _ = join.join();
        }
    }
}

/// A worker's loop: runs each chain it is sent over the rest of its block and stages it, gives it
/// back and wakes the render thread, until the pool is dropped.
fn work(shared: &Shared, mut jobs: rtrb::Consumer<Job>, mut replies: rtrb::Producer<Reply>) {
    let mut realtime: Option<suhina_realtime::RealTime> = None;
    while !shared.stop.load(Ordering::Acquire) {
        let frames = shared.real_time.load(Ordering::Acquire);
        if frames > 0 && realtime.is_none() {
            realtime = Some(suhina_realtime::RealTime::join(frames, shared.sample_rate));
        }
        let Ok(Job { index, mut chain, frames, context, reply_to }) = jobs.pop() else {
            std::thread::park();
            if let Some(rt) = realtime.as_mut() {
                rt.waited();
            }
            continue;
        };
        let started = Instant::now();
        let completed = !chain.advance(frames, &context);
        if completed {
            chain.complete(frames, &context);
            chain.stage(frames);
        }
        // Never full: the render thread takes each reply before it sends this worker another job.
        let _ = replies.push(Reply { index, chain, completed });
        reply_to.unpark();
        if let Some(rt) = realtime.as_mut() {
            rt.worked(started.elapsed());
        }
    }
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
