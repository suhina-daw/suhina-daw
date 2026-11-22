// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stretched clips rendered ahead of the audio thread (ADR 0003).
//!
//! A stretch voice costs little on average but unevenly: a restart before an onset or a stretcher
//! settling can take a large share of one audio block. So the voices of arrangement clips live on
//! a worker ([`StretchWorker`]) that keeps a lock-free ring per voice filled about
//! [`AHEAD_SECONDS`] ahead, and the audio thread ([`AheadReader`]) only copies finished frames.
//!
//! Timing never slips: the reader asks for a voice's frames by their position in the clip. If the
//! worker has fallen behind, the missing frames play as silence and are counted, and frames that
//! arrive late are dropped, so the clip carries on in time. A voice that has fallen behind by
//! more than its ring holds jumps ahead instead of rendering what would only be dropped (a
//! handshake per voice, like the rewind's below).
//!
//! The worker renders the voices playing or due within a ring's time first, and leaves the
//! others until those are full, and the voices beyond [`HORIZON_RINGS`] rings alone: a project
//! with hundreds of stretched clips kept the ones playing waiting behind all the others after
//! every start of the transport, and their beginnings were lost.
//!
//! Moving the voices (the transport stopping rewinds them; a new graph can start them anywhere)
//! is a lock-free handshake: the audio side requests a new generation with the arrangement frame
//! to move to, the worker stops pushing and says so, the audio side empties the rings, and the
//! worker moves its voices there and fills the rings again.
//!
//! A new graph (an edit, ADR 0005) takes over the voices of the clips that did not change from
//! the graph it replaces, with their stretchers and their rings, as a chain takes over its
//! running devices: the worker thread hands each such voice's writer over when the new graph's
//! worker joins it (`StretchWorker::adopt_from`), and the audio thread hands the ring's reading
//! end over when it swaps the graph in (`AheadReader::adopt_from`). The two agree through one
//! slot per voice that only one of them can claim first, so a ring's two ends always meet:
//! a voice the worker had not handed over by the swap starts afresh instead. Without it every
//! edit started every stretcher again and filled the rings of the clips playing on the caller's
//! thread first (#743).
//!
//! Without a worker thread (offline rendering) the reader fills the rings itself just before it
//! reads them, through the same voices and rings, so offline renders stay the test oracle for
//! real-time playback (AGENTS.md rule 4).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::launcher::Preparer;
use crate::stretch::StretchVoice;

/// Seconds of stretched audio each voice keeps ready.
const AHEAD_SECONDS: f64 = 0.5;
/// Project frames a voice renders per call.
const BLOCK_FRAMES: usize = 1024;
/// Most threads the stretch work is shared among ([`StretchThread`]).
pub(crate) const MAX_SHARDS: usize = 4;
/// How long the worker thread sleeps when every ring is full.
const IDLE: Duration = Duration::from_millis(1);
/// How far past the playhead, in rings' lengths, voices are rendered ahead at all.
const HORIZON_RINGS: u64 = 4;
/// A voice's handover slot ([`Shared::adopted`]) the audio side closed at the swap: too late for
/// the worker to hand the voice over.
const CLOSED: u64 = u64::MAX;

/// Mixed into a twin voice's key ([`AheadReader::new`]), so a twin is handed over only to the next
/// graph's twin of the same clip.
const TWIN_KEY: u64 = 0x9e37_79b9_7f4a_7c15;

/// The rings made so far, so each has its own number (a voice's handover names the ring).
static RINGS: AtomicU64 = AtomicU64::new(1);

/// How long a clip's new voice takes over from the one it replaces (an edit of how the clip
/// plays, such as its tempo or transpose, while it plays): the two sound at different places of
/// the clip, and cutting from one to the other clicked at each step of a drag.
const FADE_SECONDS: f64 = 0.02;

/// Frames from the start point each voice must have rendered before the transport rolls.
const START_FRAMES: u64 = 4 * BLOCK_FRAMES as u64;
/// What a voice that fell behind jumps past the frame it was asked for, in blocks: time for the
/// worker to render up to there before it is due.
const JUMP_BLOCKS: u64 = 4;

/// One voice's jump ahead, between the two sides: `requested` by the audio side with the clip
/// frame `to`, `paused` once the worker has stopped pushing for it, `drained` once the audio side
/// has emptied the voice's ring for it.
#[derive(Default)]
struct Jump {
    requested: AtomicU64,
    paused: AtomicU64,
    drained: AtomicU64,
    to: AtomicU64,
}

/// State both sides share.
#[derive(Default)]
struct Shared {
    /// Rewind generations: `requested` by the audio side, `paused` once the worker has stopped
    /// pushing for it, `drained` once the audio side has emptied every ring for it.
    requested: AtomicU64,
    /// One per shard of the worker ([`StretchWorker::split`]): each says it has stopped pushing.
    paused: [AtomicU64; MAX_SHARDS],
    drained: AtomicU64,
    /// How many shards the worker was split into (0 before a split: one).
    shards: std::sync::atomic::AtomicUsize,
    /// The arrangement frame the voices move to for the requested generation.
    seek_to: AtomicU64,
    /// Frames the audio thread needed that were not ready.
    underruns: AtomicU64,
    /// The arrangement frame the audio side reads at (or moves to): which voices are due.
    playhead: AtomicU64,
    /// Each voice's jump ahead, in the voices' order.
    jumps: Vec<Jump>,
    /// Each voice's handover from the graph before: 0 while open, the number of the ring the
    /// worker handed over to it, or [`CLOSED`] once the audio side swapped the graph in without
    /// one. Whichever side writes it first decides.
    adopted: Vec<AtomicU64>,
    /// Each voice's jump generation on the graph before when the worker handed it over: a jump
    /// that graph asked for after it is one the new writer did not follow.
    handed_jump: Vec<AtomicU64>,
    /// Each voice's index in the graph before, published with the handover: the swap finds the
    /// old reader at once instead of searching every one (hundreds of voices on the audio
    /// thread).
    handed_from: Vec<AtomicU64>,
}

impl Shared {
    /// How many shards hold the voices: each must say it paused for a rewind.
    fn shard_count(&self) -> usize {
        self.shards.load(Ordering::Acquire).clamp(1, MAX_SHARDS)
    }

    /// Whether every shard has stopped pushing for rewind generation `requested`.
    fn all_paused(&self, requested: u64) -> bool {
        self.paused.iter().take(self.shard_count()).all(|p| p.load(Ordering::Acquire) == requested)
    }
}

/// One voice on the worker side.
struct Writer {
    /// Its index among the graph's voices: its jump, handover slot and reader.
    index: usize,
    /// Its clip's id: which stretch thread renders it ([`StretchWorker::split`]).
    clip: u32,
    voice: StretchVoice,
    producer: Producer<f32>,
    /// What the voice plays ([`voice_key`] of its clip): equal keys render the same frames.
    key: u64,
    /// The number of the ring `producer` writes.
    ring: u64,
    /// The clip's first arrangement frame.
    start: u64,
    /// Frames of the clip pushed so far, and the clip's length in frames.
    produced: u64,
    length: u64,
    /// The last jump ahead followed.
    jumped: u64,
    /// Where the voice is to move before it renders next: moving it at once built and started
    /// every stretcher of the project, the far ones too, which held an edit up half a second.
    seek_to: Option<u64>,
    /// A clip's second voice for a loop's wrap ([`AheadReader::new`]): it renders only where a
    /// jump moves it, and rests at its end otherwise.
    twin: bool,
}

/// One voice on the audio side.
struct Reader {
    consumer: Consumer<f32>,
    /// The number of the ring `consumer` reads.
    ring: u64,
    start: u64,
    length: u64,
    /// The clip frame at the head of the ring.
    next: u64,
    /// Rewound, and nothing has arrived since: the silence until the worker catches up is the
    /// jump's, not an underrun.
    rewound: bool,
    /// The jump ahead asked for and not yet drained, if any.
    jumping: Option<u64>,
    /// How many blocks past the playhead its next jump goes: [`JUMP_BLOCKS`], doubled by each
    /// jump that still left it behind (up to what the ring holds), and back to [`JUMP_BLOCKS`]
    /// once a read is whole. A voice whose start-up (its stretcher built and seeked) takes longer
    /// than one jump's lead never caught up with a fixed one: each jump landed behind again and
    /// the clip stayed silent until the transport stopped.
    jump_blocks: u64,
    /// Its last jump landed ahead of the playhead, so a voice behind again since was late
    /// because the lead was too short: only then does the next jump go further. A jump that
    /// completed after the playhead had already passed its target (the worker stalled) says
    /// nothing about the lead, and doubling on it pushed the voice's return late.
    landed_ahead: bool,
    /// Its track is heard (not muted, not left out by a solo): a start waits for it.
    heard: bool,
    /// The clip the voice plays, by its id: a new graph's voice for the same clip takes over
    /// from this one by a fade when it cannot take over its ring.
    clip: u32,
    /// The ring of the voice this one replaced, while its sound fades out
    /// ([`AheadReader::adopt_from`]); a ring of nothing otherwise. It goes with the graph,
    /// never freed on the audio thread.
    fade: Consumer<f32>,
    /// The arrangement frame at the head of `fade`.
    fade_next: u64,
    /// Frames of the fade left; 0 when there is none.
    fade_left: u64,
    /// The fade has begun: until this voice's own frames arrive, the old one plays on alone.
    fade_started: bool,
    /// Its ring went to a voice of the next graph.
    handed: bool,
    /// The clip's second voice, when it plays across a loop's wrap: one voice plays up to the
    /// wrap while the other waits at the loop start, and they take turns (#1268).
    twin: Option<usize>,
    /// This voice's clip plays from its twin now.
    on_twin: bool,
    /// This is a twin: it is read only through its clip's first voice.
    is_twin: bool,
}

/// Renders stretched clips ahead into the rings the [`AheadReader`] reads. A
/// [`crate::Controller`] runs it on its own thread (`Controller::run_stretch_worker`).
pub struct StretchWorker {
    writers: Vec<Writer>,
    shared: Arc<Shared>,
    generation: u64,
    scratch: Vec<f32>,
    /// A ring's length in frames.
    ring: u64,
    /// Which shard of its graph's voices it renders ([`StretchWorker::split`]); 0 when whole.
    shard: usize,
}

impl StretchWorker {
    /// Does one round of work: follows a rewind request, or renders one block for every voice
    /// whose ring has room. Returns whether it did anything.
    pub fn work(&mut self) -> bool {
        self.work_due(false)
    }

    /// [`StretchWorker::work`], for the voices due within a ring's time only when `due_only`.
    fn work_due(&mut self, due_only: bool) -> bool {
        let requested = self.shared.requested.load(Ordering::Acquire);
        if requested != self.generation {
            // Stop pushing, wait for the audio side to empty the rings, then rewind.
            if let Some(paused) = self.shared.paused.get(self.shard) {
                paused.store(requested, Ordering::Release);
            }
            if self.shared.drained.load(Ordering::Acquire) != requested {
                return false;
            }
            self.move_to(self.shared.seek_to.load(Ordering::Acquire));
            self.generation = requested;
            return true;
        }
        let playhead = self.shared.playhead.load(Ordering::Acquire);
        // The voices playing or due within a ring first; the later ones only once those are full.
        let urgent = playhead.saturating_add(self.ring);
        let horizon = playhead.saturating_add(self.ring * HORIZON_RINGS);
        let mut did = false;
        let passes: &[u64] = if due_only { &[urgent] } else { &[urgent, horizon] };
        for &pass in passes {
            for writer in self.writers.iter_mut() {
                let Some(jump) = self.shared.jumps.get(writer.index) else { continue };
                // A voice behind the playhead is done, unless it was moved for a loop's wrap: it
                // has a jump to follow, or frames to render from where it jumped to.
                let moved = jump.requested.load(Ordering::Acquire) != writer.jumped || writer.produced < writer.length;
                if writer.start > pass || (writer.start.saturating_add(writer.length) < playhead && !moved) {
                    continue;
                }
                if !follow_jump(writer, jump) {
                    continue;
                }
                did |= render_block(writer, &mut self.scratch);
            }
            if did {
                break;
            }
        }
        did
    }

    /// Takes over from `before`, the worker of the graph this one replaces, the voices of the
    /// clips that did not change: each one's stretcher, the frames it rendered and its ring's
    /// writing end, in exchange for this graph's fresh ones, which `before` then leaves alone.
    /// Nothing while either has a rewind under way, nor a voice with a jump under way. Runs on
    /// the worker thread as the worker joins it; allocates a map of `before`'s voices.
    fn adopt_from(&mut self, before: &mut StretchWorker) {
        if before.shared.requested.load(Ordering::Acquire) != before.generation
            || self.shared.requested.load(Ordering::Acquire) != self.generation
        {
            return;
        }
        let mut by_key: std::collections::HashMap<u64, usize> =
            before.writers.iter().enumerate().map(|(index, w)| (w.key, index)).collect();
        for writer in self.writers.iter_mut() {
            let index = writer.index;
            let Some(old_index) = by_key.remove(&writer.key) else { continue };
            let (Some(old), Some(slot)) = (before.writers.get_mut(old_index), self.shared.adopted.get(index)) else {
                continue;
            };
            // A voice with a jump under way is handed over too: the new graph asks for that jump
            // again (`AheadReader::adopt_from`), as the handover says the writer had not followed
            // it. Left out, it started afresh, and its new voice had to jump as well; with a new
            // graph every 30 ms (a dragged envelope) and a jump taking two device blocks, no jump
            // ever finished and the clip stayed silent while the drag went on.
            if (old.start, old.length) != (writer.start, writer.length) {
                continue;
            }
            // The jump the voice is at now, published with the claim below.
            if let Some(handed_jump) = self.shared.handed_jump.get(index) {
                handed_jump.store(old.jumped, Ordering::Release);
            }
            if let Some(handed_from) = self.shared.handed_from.get(index) {
                handed_from.store(old.index as u64, Ordering::Release);
            }
            // Claimed first, or the audio side has swapped the graph in already: then this
            // voice starts afresh, as its reader keeps its own ring.
            if slot.compare_exchange(0, old.ring, Ordering::AcqRel, Ordering::Acquire).is_err() {
                continue;
            }
            std::mem::swap(&mut writer.voice, &mut old.voice);
            std::mem::swap(&mut writer.producer, &mut old.producer);
            std::mem::swap(&mut writer.ring, &mut old.ring);
            writer.produced = old.produced;
            writer.seek_to = old.seek_to.take();
            if let Some(jump) = self.shared.jumps.get(index) {
                writer.jumped = jump.requested.load(Ordering::Acquire);
            }
            // The fresh voice it got back renders nothing: its graph is about to go.
            old.produced = old.length;
        }
    }

    /// Shares the voices out among `shards` workers (at most [`MAX_SHARDS`]), one for each
    /// stretch thread, by their clips' ids: ids run on from one clip to the next, so a multitrack's
    /// clips are shared out evenly (by the voices' keys, a hash, eight clips on two threads went
    /// seven to one), and a clip stays on its thread through its edits, so the next graph's voice
    /// of it, with the same key, is on the thread that renders it now and is taken over there
    /// (`adopt_from`). Every shard
    /// takes part in a rewind (each says it paused), an empty one too. Before any thread has it.
    pub(crate) fn split(mut self, shards: usize) -> Vec<StretchWorker> {
        let count = shards.clamp(1, MAX_SHARDS);
        self.shared.shards.store(count, Ordering::Release);
        if count == 1 {
            return vec![self];
        }
        let mut parts: Vec<StretchWorker> = (0..count)
            .map(|shard| StretchWorker {
                writers: Vec::new(),
                shared: Arc::clone(&self.shared),
                generation: self.generation,
                scratch: vec![0.0; BLOCK_FRAMES * 2],
                ring: self.ring,
                shard,
            })
            .collect();
        for writer in self.writers.drain(..) {
            let shard = writer.clip as usize % count;
            if let Some(part) = parts.get_mut(shard) {
                part.writers.push(writer);
            }
        }
        parts
    }

    /// Whether the graph this worker renders for is gone: only its shards still hold the state
    /// the reader shared with them.
    fn orphaned(&self) -> bool {
        Arc::strong_count(&self.shared) <= self.shared.shard_count()
    }

    /// Works until every ring is full.
    pub fn fill(&mut self) {
        while self.work() {}
    }

    /// Works until the rings of the voices due within a ring's time are full: what a new graph
    /// needs before it plays. The rest is the worker thread's (filling every voice here held the
    /// window up for over a second in a project with hundreds of stretched clips).
    fn fill_due(&mut self) {
        while self.work_due(true) {}
    }

    /// Moves every voice to arrangement frame `to`: the clips playing there to that point, the
    /// later ones to their start.
    fn move_to(&mut self, to: u64) {
        for writer in self.writers.iter_mut() {
            let Some(jump) = self.shared.jumps.get(writer.index) else { continue };
            // A twin rests at its end again: its clip's first voice plays from here.
            let local = if writer.twin { writer.length } else { to.saturating_sub(writer.start).min(writer.length) };
            writer.seek_to = (local < writer.length).then_some(local);
            writer.produced = local;
            // A jump asked for before the rewind is void: the rewind drained it.
            writer.jumped = jump.requested.load(Ordering::Acquire);
        }
    }
}

/// Follows a jump ahead the audio side asked `writer`'s voice for. Returns whether the voice may
/// render now (no jump under way).
fn follow_jump(writer: &mut Writer, jump: &Jump) -> bool {
    let requested = jump.requested.load(Ordering::Acquire);
    if requested == writer.jumped {
        return true;
    }
    // Stop pushing, wait for the audio side to empty the ring, then jump.
    jump.paused.store(requested, Ordering::Release);
    if jump.drained.load(Ordering::Acquire) != requested {
        return false;
    }
    let to = jump.to.load(Ordering::Acquire).min(writer.length);
    writer.seek_to = (to < writer.length).then_some(to);
    writer.produced = to;
    writer.jumped = requested;
    true
}

/// Renders one block of `writer`'s voice into its ring when there is room. Returns whether it did.
fn render_block(writer: &mut Writer, scratch: &mut [f32]) -> bool {
    let left = writer.length.saturating_sub(writer.produced);
    let frames = (BLOCK_FRAMES as u64).min(left) as usize;
    if frames == 0 || writer.producer.slots() < frames * 2 {
        return false;
    }
    let Some(block) = scratch.get_mut(..frames * 2) else { return false };
    // Built when it first renders, on the worker's thread (or offline): the far voices of a
    // project are never built until they are due.
    writer.voice.prepare();
    if let Some(at) = writer.seek_to.take() {
        writer.voice.seek(at);
    }
    writer.voice.render(block);
    if let Ok(mut chunk) = writer.producer.write_chunk(frames * 2) {
        let (first, second) = chunk.as_mut_slices();
        let (a, b) = block.split_at(first.len());
        first.copy_from_slice(a);
        second.copy_from_slice(b);
        chunk.commit_all();
    }
    writer.produced += frames as u64;
    true
}

/// Workers that can wait to join the thread.
const INBOX: usize = 8;

/// One thread of the [`StretchThread`] pool: the shard of each graph's voices it renders, and
/// (the first only) the launcher's preparers.
struct Lane {
    handle: Option<JoinHandle<()>>,
    inbox: Producer<StretchWorker>,
    #[cfg(test)]
    sync: std::sync::mpsc::Sender<std::sync::mpsc::Sender<()>>,
}

/// The threads running [`StretchWorker`]s and the launcher's [`Preparer`]s (ADR 0005): each
/// graph's worker is split into one shard per thread ([`StretchWorker::split`]), so the stretched
/// clips of a multitrack are rendered on several cores at once (one thread took them all, and
/// eight songs warped together fell behind and stuttered). Each shard joins its thread through
/// [`StretchThread::add`] and leaves once its graph has been dropped; the first thread also runs
/// the preparers ([`StretchThread::add_preparer`]), which leave once swapped out. Dropping the
/// handle stops the threads.
pub(crate) struct StretchThread {
    stop: Arc<AtomicBool>,
    lanes: Vec<Lane>,
    preparers: Producer<Preparer>,
}

/// How many threads the stretch work is shared among: the cores but two (the audio thread and the
/// window's), from one to [`MAX_SHARDS`].
fn stretch_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(2)).clamp(1, MAX_SHARDS)
}

impl StretchThread {
    pub(crate) fn spawn() -> std::io::Result<Self> {
        Self::spawn_with(stretch_threads())
    }

    /// [`StretchThread::spawn`] with `count` threads.
    pub(crate) fn spawn_with(count: usize) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (preparers, incoming_preparers) = RingBuffer::<Preparer>::new(INBOX);
        let mut incoming_preparers = Some(incoming_preparers);
        let mut lanes = Vec::new();
        for number in 0..count.clamp(1, MAX_SHARDS) {
            let stop_flag = Arc::clone(&stop);
            let (inbox, incoming) = RingBuffer::<StretchWorker>::new(INBOX);
            let preparers_here = incoming_preparers.take();
            #[cfg(test)]
            let (sync, requests) = std::sync::mpsc::channel::<std::sync::mpsc::Sender<()>>();
            let name = if number == 0 { "suhina-stretch".to_string() } else { format!("suhina-stretch-{number}") };
            let spawned = std::thread::Builder::new().name(name).spawn(move || {
                #[cfg(test)]
                run_lane(&stop_flag, incoming, preparers_here, requests);
                #[cfg(not(test))]
                run_lane(&stop_flag, incoming, preparers_here);
            });
            let handle = match spawned {
                Ok(handle) => handle,
                Err(error) => {
                    // The threads already started would otherwise run for ever: nothing holds
                    // their stop flag once this returns.
                    stop_lanes(&stop, &mut lanes);
                    return Err(error);
                }
            };
            lanes.push(Lane {
                handle: Some(handle),
                inbox,
                #[cfg(test)]
                sync,
            });
        }
        Ok(Self { stop, lanes, preparers })
    }

    /// Waits until all work queued before this call can make no more progress without the
    /// renderer, on every thread. Only the test driver calls this, between audio blocks, never
    /// `process`.
    #[cfg(test)]
    pub(crate) fn sync(&self) {
        let mut waits = Vec::new();
        for lane in &self.lanes {
            let (done, finished) = std::sync::mpsc::channel();
            lane.sync.send(done).expect("stretch thread accepts the sync request");
            lane.handle.as_ref().expect("stretch thread is running").thread().unpark();
            waits.push(finished);
        }
        for finished in waits {
            finished.recv().expect("stretch thread finishes the queued work");
        }
    }

    /// Hands a graph's worker to the threads, a shard to each.
    pub(crate) fn add(&mut self, worker: StretchWorker) -> Result<(), crate::ControlError> {
        // Room on every thread first: a graph is all there or not at all.
        if self.lanes.iter().any(|lane| lane.inbox.is_full()) {
            return Err(crate::ControlError::StretchBusy);
        }
        let shards = worker.split(self.lanes.len());
        for (lane, shard) in self.lanes.iter_mut().zip(shards) {
            lane.inbox.push(shard).map_err(|_| crate::ControlError::StretchBusy)?;
        }
        Ok(())
    }

    /// Hands a graph's launcher preparer to the first thread.
    pub(crate) fn add_preparer(&mut self, preparer: Preparer) -> Result<(), crate::ControlError> {
        self.preparers.push(preparer).map_err(|_| crate::ControlError::StretchBusy)
    }
}

/// One stretch thread's loop: takes the shards (and, on the first, the preparers) handed to it,
/// works them round by round, and sleeps a little when nothing had room.
fn run_lane(
    stop: &AtomicBool,
    mut incoming: Consumer<StretchWorker>,
    mut incoming_preparers: Option<Consumer<Preparer>>,
    #[cfg(test)] requests: std::sync::mpsc::Receiver<std::sync::mpsc::Sender<()>>,
) {
    let mut workers: Vec<StretchWorker> = Vec::new();
    let mut preparing: Vec<Preparer> = Vec::new();
    #[cfg(test)]
    let mut pending = None;
    while !stop.load(Ordering::Relaxed) {
        // Observe the request before draining the inboxes: a test's acknowledgement must include
        // the workers it queued before asking to synchronize.
        #[cfg(test)]
        if pending.is_none() {
            pending = requests.try_recv().ok();
        }
        while let Ok(mut worker) = incoming.pop() {
            // The graph it replaces is the newest one here: its voices carry over.
            if let Some(before) = workers.last_mut() {
                worker.adopt_from(before);
            }
            workers.push(worker);
        }
        if let Some(incoming_preparers) = incoming_preparers.as_mut() {
            while let Ok(mut preparer) = incoming_preparers.pop() {
                // The same: the voices the newest graph has may be taken over at the swap.
                if let Some(before) = preparing.last() {
                    preparer.follow(before);
                }
                preparing.push(preparer);
            }
        }
        // A worker whose reader (its graph) is gone has nobody to render for.
        workers.retain(|w| !w.orphaned());
        preparing.retain(|p| !p.finished());
        let mut did = false;
        for worker in &mut workers {
            did |= worker.work();
        }
        // One launcher voice a round (a few milliseconds), so the rings above keep filling: one
        // the audio side asked for, else the newest graph's next.
        let asked = preparing.iter_mut().rev().any(|p| p.work(true));
        did |= asked || preparing.iter_mut().rev().any(|p| p.work(false));
        if !did {
            #[cfg(test)]
            {
                if let Some(done) = pending.take() {
                    let _ = done.send(());
                }
                // A sync request wakes the idle thread without waiting for a timer. Tests that do
                // not synchronize still get the normal idle cadence.
                std::thread::park_timeout(IDLE);
            }
            #[cfg(not(test))]
            std::thread::sleep(IDLE);
        }
    }
}

impl Drop for StretchThread {
    fn drop(&mut self) {
        stop_lanes(&self.stop, &mut self.lanes);
    }
}

/// Stops the threads of `lanes` and waits for them to finish.
fn stop_lanes(stop: &AtomicBool, lanes: &mut [Lane]) {
    stop.store(true, Ordering::Relaxed);
    for lane in lanes {
        if let Some(handle) = lane.handle.take() {
            #[cfg(test)]
            handle.thread().unpark();
            // A worker that panicked has nothing left to clean up.
            let _ = handle.join();
        }
    }
}

/// The audio thread's side: reads each voice's frames by their position in the clip.
pub(crate) struct AheadReader {
    readers: Vec<Reader>,
    shared: Arc<Shared>,
    /// A ring's length in frames: how long before a loop's wrap the voices at its start are
    /// moved there.
    ring_frames: u64,
    /// The length of a fade from a replaced voice, in frames ([`FADE_SECONDS`]).
    fade_frames: u64,
    /// The worker, while no thread has taken it: the reader then fills the rings itself.
    inline: Option<StretchWorker>,
}

impl AheadReader {
    /// Splits `voices` (each with its clip's start and length in arrangement frames, its
    /// [`voice_key`] and its clip's id) into a reader and its worker, with a ring per voice
    /// holding [`AHEAD_SECONDS`] at `sample_rate`.
    ///
    /// `twins` are second voices of clips that play across a loop's wrap (the index of the clip's
    /// voice in `voices`, and another voice of the same clip): the voice playing up to the wrap
    /// is read to its last frame, so it cannot be moved to the loop start ahead; its twin is, and
    /// the two take turns at each wrap. A twin renders only once moved there.
    pub(crate) fn new(
        voices: Vec<(StretchVoice, u64, u64, u64, u32)>,
        twins: Vec<(usize, StretchVoice)>,
        sample_rate: u32,
    ) -> Self {
        let frames = ((AHEAD_SECONDS * f64::from(sample_rate)) as usize).max(2 * BLOCK_FRAMES);
        let firsts = voices.len();
        // Each twin as its clip's voice: the same place, its own key (a twin is handed over to
        // the next graph's twin), its clip's id.
        let twins: Vec<(StretchVoice, u64, u64, u64, u32, usize)> = twins
            .into_iter()
            .filter_map(|(of, voice)| {
                let &(_, start, length, key, clip) = voices.get(of)?;
                Some((voice, start, length, key ^ TWIN_KEY, clip, of))
            })
            .collect();
        let count = firsts + twins.len();
        let jumps = (0..count).map(|_| Jump::default()).collect();
        let adopted = (0..count).map(|_| AtomicU64::new(0)).collect();
        let handed_jump = (0..count).map(|_| AtomicU64::new(0)).collect();
        let handed_from = (0..count).map(|_| AtomicU64::new(0)).collect();
        let shared = Arc::new(Shared { jumps, adopted, handed_jump, handed_from, ..Shared::default() });
        let mut readers = Vec::with_capacity(count);
        let mut writers = Vec::with_capacity(count);
        let all =
            voices.into_iter().map(|(voice, start, length, key, clip)| (voice, start, length, key, clip, None)).chain(
                twins
                    .into_iter()
                    .map(|(voice, start, length, key, clip, of)| (voice, start, length, key, clip, Some(of))),
            );
        for (voice, start, length, key, clip, twin_of) in all {
            let (producer, consumer) = RingBuffer::new(frames * 2);
            let ring = RINGS.fetch_add(1, Ordering::Relaxed);
            // A ring of nothing, for the place a replaced voice's ring takes at a swap.
            let (_, fade) = RingBuffer::new(0);
            let is_twin = twin_of.is_some();
            // A twin rests at its end until a wrap moves it.
            let at = if is_twin { length } else { 0 };
            readers.push(Reader {
                consumer,
                ring,
                start,
                length,
                next: at,
                rewound: false,
                jumping: None,
                jump_blocks: JUMP_BLOCKS,
                landed_ahead: false,
                heard: true,
                clip,
                fade,
                fade_next: 0,
                fade_left: 0,
                fade_started: false,
                handed: false,
                twin: None,
                on_twin: false,
                is_twin,
            });
            writers.push(Writer {
                index: writers.len(),
                clip,
                voice,
                producer,
                key,
                ring,
                start,
                produced: at,
                length,
                jumped: 0,
                seek_to: None,
                twin: is_twin,
            });
            let index = readers.len().saturating_sub(1);
            if let Some(first) = twin_of.and_then(|of| readers.get_mut(of)) {
                first.twin = Some(index);
            }
        }
        let worker = StretchWorker {
            writers,
            shared: Arc::clone(&shared),
            generation: 0,
            scratch: vec![0.0; BLOCK_FRAMES * 2],
            ring: frames as u64,
            shard: 0,
        };
        let fade_frames = ((FADE_SECONDS * f64::from(sample_rate)) as u64).max(1);
        Self { readers, shared, ring_frames: frames as u64, fade_frames, inline: Some(worker) }
    }

    /// Whether there are no stretched clips to read.
    pub(crate) fn is_empty(&self) -> bool {
        self.readers.is_empty()
    }

    /// Hands the worker to a thread of its own, with the rings of the voices due already full.
    /// `None` once taken.
    pub(crate) fn take_worker(&mut self) -> Option<StretchWorker> {
        let mut worker = self.inline.take()?;
        worker.fill_due();
        Some(worker)
    }

    /// Hands the worker to a thread of its own as it is: the thread takes the unchanged clips'
    /// voices over from the graph before and renders the rest ahead of the swap, so nothing is
    /// rendered on the caller's thread (#743). `None` once taken.
    pub(crate) fn take_worker_unfilled(&mut self) -> Option<StretchWorker> {
        self.inline.take()
    }

    /// Takes over from `before`, the graph swapped out, the rings of the voices the worker
    /// handed over: each one's reading end and where it is, so the clip plays on without a
    /// break. A voice not handed over yet is closed to it and starts afresh. On the audio
    /// thread at the swap; never allocates.
    ///
    /// A voice of a clip that changed how it plays (its tempo, its transpose) cannot be handed
    /// over: its new voice plays from another place of the clip. It fades in over the old one's
    /// ring instead, which plays on alone until the new voice's frames arrive.
    ///
    /// `shift` is how far the new graph's arrangement frames are from the old one's at the swap
    /// (the new playhead frame less the old): a tempo change moves every frame after the beat
    /// that plays, and a fade from an old voice reads its ring at the old frames moved by it.
    pub(crate) fn adopt_from(&mut self, before: &mut AheadReader, shift: i64) {
        let mut fresh = false;
        for (index, (reader, slot)) in self.readers.iter_mut().zip(&self.shared.adopted).enumerate() {
            let ring = match slot.compare_exchange(0, CLOSED, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    fresh = true;
                    continue;
                }
                Err(ring) => ring,
            };
            let Some(old_index) =
                self.shared.handed_from.get(index).and_then(|i| usize::try_from(i.load(Ordering::Acquire)).ok())
            else {
                continue;
            };
            // The index is the old graph's; its ring confirms it is the reader handed over.
            let Some(old) = before.readers.get_mut(old_index).filter(|r| r.ring == ring) else { continue };
            old.handed = true;
            reader.handed = true;
            std::mem::swap(&mut reader.consumer, &mut old.consumer);
            std::mem::swap(&mut reader.ring, &mut old.ring);
            reader.next = old.next;
            // The lead its jumps go by comes along with where it is: a voice still catching up
            // after a jump keeps its longer lead instead of starting over from the shortest.
            reader.jump_blocks = old.jump_blocks;
            reader.landed_ahead = old.landed_ahead;
            reader.rewound = old.rewound;
            reader.on_twin = old.on_twin;
            reader.jumping = None;
            // A jump the old graph asked for after the handover went to the old writer, not to
            // the one rendering this ring now: the ring's frames no longer start at `next`. This
            // graph jumps the voice again, to where the old one meant to be, which lines the two
            // ends up.
            let handed = self.shared.handed_jump.get(index).map(|j| j.load(Ordering::Acquire));
            let Some(old_jump) = before.shared.jumps.get(old_index) else { continue };
            if handed == Some(old_jump.requested.load(Ordering::Acquire)) {
                continue;
            }
            let to = if old.jumping.is_some() { old_jump.to.load(Ordering::Acquire) } else { old.next };
            if let Some(jump) = self.shared.jumps.get(index) {
                let requested = jump.requested.load(Ordering::Acquire) + 1;
                jump.to.store(to, Ordering::Release);
                jump.requested.store(requested, Ordering::Release);
                reader.jumping = Some(requested);
            }
        }
        if fresh {
            self.fade_from(before, shift);
        }
        // A clip that played from its twin goes on there only when that ring came along too.
        for index in 0..self.readers.len() {
            let Some(twin) = self.readers.get(index).and_then(|r| r.twin) else { continue };
            let twin_handed = self.readers.get(twin).is_some_and(|t| t.handed);
            if let Some(reader) = self.readers.get_mut(index)
                && reader.on_twin
                && !twin_handed
            {
                reader.on_twin = false;
            }
        }
        // `handed` marks this swap only.
        for reader in &mut self.readers {
            reader.handed = false;
        }
    }

    /// Starts the fades of [`AheadReader::adopt_from`]: each voice that starts afresh takes the
    /// ring of the old voice of its clip, when that one was playing where it should be.
    fn fade_from(&mut self, before: &mut AheadReader, shift: i64) {
        // An old frame as the new graph counts it.
        let moved = |frame: u64| frame.saturating_add_signed(shift);
        let settled = before.shared.drained.load(Ordering::Acquire) == before.shared.requested.load(Ordering::Acquire);
        if !settled {
            return;
        }
        for reader in &mut self.readers {
            if reader.handed || reader.is_twin {
                continue;
            }
            let Some(old) = before.readers.iter_mut().find(|r| r.clip == reader.clip && !r.handed && !r.is_twin) else {
                continue;
            };
            if old.jumping.is_some() || old.rewound {
                continue;
            }
            old.handed = true;
            // A voice still waiting for its own frames: the sound heard is the one it fades from.
            if old.fade_left > 0 && !old.fade_started {
                std::mem::swap(&mut reader.fade, &mut old.fade);
                reader.fade_next = moved(old.fade_next);
            } else {
                std::mem::swap(&mut reader.fade, &mut old.consumer);
                // Its ring holds the frames from here on, counted from the old graph's frames: a
                // tempo change (120 to 90 BPM moves a beat's frame a third later) made the fade
                // skip frames of the old sound at the swap, a click.
                reader.fade_next = moved(old.start.saturating_add(old.next));
            }
            reader.fade_left = self.fade_frames;
            reader.fade_started = false;
        }
    }

    /// Asks for the voices to continue from arrangement frame `to`; until the worker has moved
    /// them, reads are silence.
    pub(crate) fn move_to(&mut self, to: u64) {
        // The sound moves anyway: no fade from where it was.
        for reader in &mut self.readers {
            reader.fade_left = 0;
        }
        self.move_voices(to);
    }

    /// [`AheadReader::move_to`], for a graph swapped in off where its voices were rendered for:
    /// the fades from the old voices play on, as their rings are where the playhead is, and
    /// cover the new voices' silence until the worker has moved them.
    pub(crate) fn move_voices(&mut self, to: u64) {
        self.shared.seek_to.store(to, Ordering::Release);
        self.shared.playhead.store(to, Ordering::Release);
        self.shared.requested.fetch_add(1, Ordering::AcqRel);
        self.service();
    }

    /// Whether the voices playing at arrangement frame `at`, or starting soon after, have their
    /// first frames from there rendered: what a start waits for. Always, without a worker
    /// thread (the reader renders them as it reads).
    pub(crate) fn ready_at(&self, at: u64) -> bool {
        if self.inline.is_some() {
            return true;
        }
        if self.shared.drained.load(Ordering::Acquire) != self.shared.requested.load(Ordering::Acquire) {
            return false;
        }
        let lead = START_FRAMES;
        // A twin rests until a wrap moves it: nothing waits for it.
        self.readers.iter().filter(|r| r.heard && !r.is_twin).all(|reader| {
            let end = reader.start.saturating_add(reader.length);
            if reader.start > at + lead || end <= at {
                return true;
            }
            let local = at.saturating_sub(reader.start);
            let needed = lead.min(reader.length - local);
            (reader.consumer.slots() / 2) as u64 >= needed
        })
    }

    /// Which voices' tracks are heard, in the voices' order: a start waits only for those, so a
    /// solo does not hold it up for the clips it silences.
    pub(crate) fn set_heard(&mut self, heard: &[bool]) {
        for (reader, heard) in self.readers.iter_mut().zip(heard) {
            reader.heard = *heard;
        }
    }

    /// Tells the worker the arrangement frame being played, so the voices due from there are
    /// rendered first.
    pub(crate) fn set_playhead(&self, frame: u64) {
        self.shared.playhead.store(frame, Ordering::Release);
    }

    /// Before a loop's wrap: once the playhead (arrangement frame `now`) is within a ring's time of
    /// the loop end, the voices that have played to their end and play again at the loop start
    /// (`start`) are moved there ahead, each by a jump of its own, so the worker renders their
    /// frames from there before the wrap reaches them: the wrap then plays them at once, as the
    /// first pass did, instead of after a silence and from their middle (a click). Never blocks
    /// or allocates.
    pub(crate) fn prepare_wrap(&mut self, start: u64, end: u64, now: u64) {
        if now.saturating_add(self.ring_frames) < end
            || self.shared.drained.load(Ordering::Acquire) != self.shared.requested.load(Ordering::Acquire)
        {
            return;
        }
        let lead = self.ring_frames;
        for index in 0..self.readers.len() {
            let Some(reader) = self.readers.get(index).filter(|r| !r.is_twin) else { continue };
            let clip_end = reader.start.saturating_add(reader.length);
            let local = if reader.start <= start && start < clip_end {
                start - reader.start
            } else if start < reader.start && reader.start < end.min(start.saturating_add(lead)) {
                0
            } else {
                continue;
            };
            // The voice the clip plays from now, and the one waiting, if it has a twin.
            let (playing, waiting) = match reader.twin {
                Some(twin) if reader.on_twin => (twin, Some(index)),
                Some(twin) => (index, Some(twin)),
                None => (index, None),
            };
            // A voice the playhead reads again before the wrap stays; its twin goes instead.
            let moved = if clip_end > now { waiting } else { Some(playing) };
            let Some(moved) = moved else { continue };
            if let (Some(reader), Some(jump)) = (self.readers.get_mut(moved), self.shared.jumps.get(moved)) {
                request_jump(reader, jump, local);
            }
        }
        for (reader, jump) in self.readers.iter_mut().zip(&self.shared.jumps) {
            complete_jump(reader, jump);
        }
    }

    /// At a loop's wrap to arrangement frame `to`: every voice goes on from there, each by a jump
    /// of its own. The voices [`AheadReader::prepare_wrap`] moved there already keep their rings
    /// and play at once, and a clip that played up to the wrap goes on from its twin, which waited
    /// there. Any other voice is silent until the worker has moved it, and that silence counts as
    /// late. Never blocks or allocates.
    pub(crate) fn wrap_to(&mut self, to: u64) {
        if self.shared.drained.load(Ordering::Acquire) != self.shared.requested.load(Ordering::Acquire) {
            self.move_to(to);
            return;
        }
        self.shared.playhead.store(to, Ordering::Release);
        for index in 0..self.readers.len() {
            let Some(reader) = self.readers.get_mut(index).filter(|r| !r.is_twin) else { continue };
            reader.fade_left = 0;
            let local = to.saturating_sub(reader.start).min(reader.length);
            let (on_twin, twin) = (reader.on_twin, reader.twin);
            let playing = match twin {
                Some(twin) if on_twin => twin,
                _ => index,
            };
            // A twin moved to the loop start ahead takes over, with its ring ready; the voice
            // that played up to the wrap waits for the next one.
            if let Some(twin) = twin {
                let waiting = if on_twin { index } else { twin };
                let ready = match (self.readers.get(waiting), self.shared.jumps.get(waiting)) {
                    (Some(r), Some(jump)) => match r.jumping {
                        Some(_) => jump.to.load(Ordering::Acquire) == local,
                        None => r.next == local && !r.rewound,
                    },
                    _ => false,
                };
                if ready {
                    if let Some(reader) = self.readers.get_mut(index) {
                        reader.on_twin = !on_twin;
                    }
                    continue;
                }
            }
            if let (Some(reader), Some(jump)) = (self.readers.get_mut(playing), self.shared.jumps.get(playing)) {
                request_jump(reader, jump, local);
            }
        }
        self.service();
    }

    /// Moves a pending rewind along, and the jumps the worker has paused for; call it before
    /// reading. Never blocks.
    pub(crate) fn service(&mut self) {
        for (reader, jump) in self.readers.iter_mut().zip(&self.shared.jumps) {
            complete_jump(reader, jump);
        }
        let requested = self.shared.requested.load(Ordering::Acquire);
        if self.shared.drained.load(Ordering::Acquire) == requested {
            return;
        }
        if let Some(worker) = self.inline.as_mut() {
            // Nothing else pushes: the reader stands in for the worker's side of the handshake.
            if let Some(paused) = worker.shared.paused.first() {
                paused.store(requested, Ordering::Release);
            }
        }
        if !self.shared.all_paused(requested) {
            return;
        }
        let to = self.shared.seek_to.load(Ordering::Acquire);
        for (reader, jump) in self.readers.iter_mut().zip(&self.shared.jumps) {
            let queued = reader.consumer.slots();
            if let Ok(chunk) = reader.consumer.read_chunk(queued) {
                chunk.commit_all();
            }
            // A twin rests at its end, as its writer does; each clip plays from its first voice.
            reader.next =
                if reader.is_twin { reader.length } else { to.saturating_sub(reader.start).min(reader.length) };
            reader.rewound = !reader.is_twin;
            reader.on_twin = false;
            // A rewind starts the voice afresh: its jumps go the usual way again.
            reader.jump_blocks = JUMP_BLOCKS;
            reader.landed_ahead = false;
            // A jump under way is void: the rewind moves the voice.
            if reader.jumping.take().is_some() {
                let requested = jump.requested.load(Ordering::Acquire);
                jump.paused.store(requested, Ordering::Release);
                jump.drained.store(requested, Ordering::Release);
            }
        }
        self.shared.drained.store(requested, Ordering::Release);
        if let Some(worker) = self.inline.as_mut() {
            worker.work();
        }
    }

    /// Fills `out` (interleaved stereo) with voice `index`'s frames from clip frame `from` on.
    /// Frames that are not ready are silence and are counted as underruns, except while a rewind
    /// is under way or has delivered nothing yet (that silence is the jump's); never blocks or
    /// allocates.
    pub(crate) fn read(&mut self, index: usize, from: u64, out: &mut [f32]) {
        // A clip playing from its twin since a wrap is read there.
        let index = match self.readers.get(index) {
            Some(Reader { twin: Some(twin), on_twin: true, .. }) => *twin,
            _ => index,
        };
        let got = self.read_ring(index, from, out);
        let fade_frames = self.fade_frames;
        if let Some(reader) = self.readers.get_mut(index)
            && reader.fade_left > 0
        {
            fade(reader, from, out, got, fade_frames);
        }
    }

    /// [`AheadReader::read`] of the voice's own ring. Returns the frames it put at the start of
    /// `out`.
    fn read_ring(&mut self, index: usize, from: u64, out: &mut [f32]) -> usize {
        out.fill(0.0);
        let wanted = (out.len() / 2) as u64;
        if let Some(reader) = self.readers.get(index) {
            self.shared.playhead.store(reader.start.saturating_add(from), Ordering::Release);
        }
        if let Some(worker) = self.inline.as_mut() {
            worker.fill();
        }
        let ready = self.shared.drained.load(Ordering::Acquire) == self.shared.requested.load(Ordering::Acquire);
        let Some(reader) = self.readers.get_mut(index) else { return 0 };
        // A rewind under way, or done with nothing rendered since: silence the transport asked
        // for, not frames the worker failed to deliver.
        if !ready {
            return 0;
        }
        let Some(jump) = self.shared.jumps.get(index) else { return 0 };
        // A jump under way: once the worker has stopped pushing, the ring is emptied and the voice
        // goes on from where it jumped to.
        let was_jumping = reader.jumping.is_some();
        if !complete_jump(reader, jump) {
            self.shared.underruns.fetch_add(wanted, Ordering::Relaxed);
            return 0;
        }
        if was_jumping {
            reader.landed_ahead = reader.next >= from;
        }
        if reader.rewound {
            if reader.consumer.slots() == 0 {
                return 0;
            }
            reader.rewound = false;
        }
        // Drop what was due before `from` (frames that arrived too late to play).
        if reader.next < from {
            let skip = ((from - reader.next) as usize).min(reader.consumer.slots() / 2);
            if let Ok(chunk) = reader.consumer.read_chunk(skip * 2) {
                chunk.commit_all();
            }
            reader.next += skip as u64;
        }
        if reader.next != from {
            self.shared.underruns.fetch_add(wanted, Ordering::Relaxed);
            // Behind by more than a block, with nothing left to drop: rendering on from there
            // would only feed frames that are dropped. Jump ahead of the playhead instead.
            // Ahead of the playhead, its ring holds frames not due yet: a voice left where a
            // loop's wrap or a swap went past it (a clip read from its first voice again while
            // that waited at the loop end). Reading on would be silent until the playhead came
            // round to it, a whole pass; it jumps back to the playhead instead.
            // Past the lead a jump of its own puts it ahead by (below), so that jump plays on.
            let lead = wanted + (reader.jump_blocks + 2) * BLOCK_FRAMES as u64;
            let ahead = reader.next > from.saturating_add(lead) && !reader.rewound;
            let behind = reader.next < from && from - reader.next > BLOCK_FRAMES as u64 && reader.consumer.slots() == 0;
            if ahead || behind {
                // Behind again after a jump that landed in time: this one goes further, within
                // the ring. The first jump, and one after a stale landing, keep the lead.
                if behind && std::mem::take(&mut reader.landed_ahead) {
                    let most = (self.ring_frames / BLOCK_FRAMES as u64).saturating_sub(2).max(JUMP_BLOCKS);
                    reader.jump_blocks = reader.jump_blocks.saturating_mul(2).min(most);
                }
                reader.landed_ahead = false;
                let requested = jump.requested.load(Ordering::Acquire) + 1;
                let to = from + wanted + reader.jump_blocks * BLOCK_FRAMES as u64;
                jump.to.store(to, Ordering::Release);
                jump.requested.store(requested, Ordering::Release);
                reader.jumping = Some(requested);
            }
            return 0;
        }
        let frames = (wanted as usize).min(reader.consumer.slots() / 2);
        if let Ok(chunk) = reader.consumer.read_chunk(frames * 2) {
            let (first, second) = chunk.as_slices();
            if let Some(dst) = out.get_mut(..first.len()) {
                dst.copy_from_slice(first);
            }
            if let Some(dst) = out.get_mut(first.len()..first.len() + second.len()) {
                dst.copy_from_slice(second);
            }
            chunk.commit_all();
        }
        reader.next += frames as u64;
        if (frames as u64) < wanted {
            self.shared.underruns.fetch_add(wanted - frames as u64, Ordering::Relaxed);
        } else {
            reader.jump_blocks = JUMP_BLOCKS;
            reader.landed_ahead = false;
        }
        frames
    }

    /// Frames needed that were not ready, since the renderer was built.
    pub(crate) fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }
}

/// Mixes the fade from a replaced voice into `out`, which holds `got` frames of `reader`'s own
/// from clip frame `from` on: until those arrive the old voice plays alone, then it fades out as
/// the new one fades in, both at equal power over `fade_frames`. A fade with nothing left to
/// play from ends. Never allocates.
fn fade(reader: &mut Reader, from: u64, out: &mut [f32], got: usize, fade_frames: u64) {
    let wanted = out.len() / 2;
    let at = reader.start.saturating_add(from);
    // Drop what was due before `at`.
    if reader.fade_next < at {
        let skip = ((at - reader.fade_next) as usize).min(reader.fade.slots() / 2);
        if let Ok(chunk) = reader.fade.read_chunk(skip * 2) {
            chunk.commit_all();
        }
        reader.fade_next += skip as u64;
    }
    if reader.fade_next != at {
        reader.fade_left = 0;
        return;
    }
    let available = reader.fade.slots() / 2;
    let frames = wanted.min(available);
    let Ok(chunk) = reader.fade.read_chunk(frames * 2) else {
        reader.fade_left = 0;
        return;
    };
    let (first, second) = chunk.as_slices();
    let old = |i: usize| first.get(i).or_else(|| second.get(i - first.len())).copied().unwrap_or(0.0);
    if !reader.fade_started && got < wanted {
        // The new voice is not there yet: the old one plays on.
        for (i, sample) in out.iter_mut().enumerate().take(frames * 2) {
            *sample = old(i);
        }
    } else {
        reader.fade_started = true;
        let length = fade_frames.max(1) as f32;
        for (frame, pair) in out.as_chunks_mut::<2>().0.iter_mut().enumerate().take(frames) {
            if reader.fade_left == 0 {
                break;
            }
            // From the old voice alone to the new one alone.
            let t = 1.0 - reader.fade_left as f32 / length;
            let (new_gain, old_gain) = (t * std::f32::consts::FRAC_PI_2).sin_cos();
            pair[0] = pair[0] * new_gain + old(frame * 2) * old_gain;
            pair[1] = pair[1] * new_gain + old(frame * 2 + 1) * old_gain;
            reader.fade_left -= 1;
        }
    }
    chunk.commit_all();
    reader.fade_next += frames as u64;
    if frames < wanted {
        reader.fade_left = 0;
    }
}

/// Asks for `reader`'s voice to go on from clip frame `local`, unless it is there already or a
/// jump there is under way.
fn request_jump(reader: &mut Reader, jump: &Jump, local: u64) {
    let local = local.min(reader.length);
    let under_way = reader.jumping.is_some() && jump.to.load(Ordering::Acquire) == local;
    if under_way || (reader.jumping.is_none() && reader.next == local && !reader.rewound) {
        return;
    }
    let requested = jump.requested.load(Ordering::Acquire) + 1;
    jump.to.store(local, Ordering::Release);
    jump.requested.store(requested, Ordering::Release);
    reader.jumping = Some(requested);
}

/// Finishes `reader`'s jump once the worker has stopped pushing for it: empties the ring and goes
/// on from where it jumped to. Returns whether no jump is left under way.
fn complete_jump(reader: &mut Reader, jump: &Jump) -> bool {
    let Some(requested) = reader.jumping else { return true };
    if jump.paused.load(Ordering::Acquire) != requested {
        return false;
    }
    let queued = reader.consumer.slots();
    if let Ok(chunk) = reader.consumer.read_chunk(queued) {
        chunk.commit_all();
    }
    reader.next = jump.to.load(Ordering::Acquire).min(reader.length);
    reader.jumping = None;
    reader.rewound = false;
    jump.drained.store(requested, Ordering::Release);
    true
}

/// What a stretch voice plays, as a number: equal for two graphs' voices that render the same
/// frames (the same clip, placed and cut the same, of the same audio, warped the same, on the
/// same tempo map), so the new graph can take the old one's voice over.
pub(crate) fn voice_key(
    clip: &suhina_model::Clip,
    region: &suhina_model::AudioRegion,
    data: &Arc<crate::AudioData>,
    (start, length): (u64, u64),
    tempo: u64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    clip.id.0.hash(&mut hasher);
    (start, length, tempo).hash(&mut hasher);
    // The decoded audio itself: new samples under the same pool key are another voice.
    (Arc::as_ptr(data) as usize).hash(&mut hasher);
    for value in [clip.start, clip.length, clip.offset] {
        value.to_bits().hash(&mut hasher);
    }
    clip.loop_range.map(|r| (r.start.to_bits(), r.end.to_bits())).hash(&mut hasher);
    region.pool_key().hash(&mut hasher);
    (clip.audio.warped, clip.audio.transpose.to_bits()).hash(&mut hasher);
    std::mem::discriminant(&clip.audio.stretch).hash(&mut hasher);
    // The mode's settings: a changed grain, scatter, slice, tail or envelope is heard at once.
    let warp = clip.audio.warp;
    (warp.slice.name(), warp.tail.name()).hash(&mut hasher);
    for value in [warp.envelope, warp.grain, warp.scatter] {
        value.to_bits().hash(&mut hasher);
    }
    for marker in &clip.audio.warp_markers {
        (marker.time.to_bits(), marker.audio_time.to_bits()).hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
#[path = "ahead_tests.rs"]
mod handover_tests;
