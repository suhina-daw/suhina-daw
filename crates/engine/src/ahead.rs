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
//! Without a worker thread (offline rendering) the reader fills the rings itself just before it
//! reads them, through the same voices and rings, so offline renders stay the test oracle for
//! real-time playback (AGENTS.md rule 4).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::stretch::StretchVoice;

/// Seconds of stretched audio each voice keeps ready.
const AHEAD_SECONDS: f64 = 0.5;
/// Project frames a voice renders per call.
const BLOCK_FRAMES: usize = 1024;
/// How long the worker thread sleeps when every ring is full.
const IDLE: Duration = Duration::from_millis(1);
/// How far past the playhead, in rings' lengths, voices are rendered ahead at all.
const HORIZON_RINGS: u64 = 4;
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
    paused: AtomicU64,
    drained: AtomicU64,
    /// The arrangement frame the voices move to for the requested generation.
    seek_to: AtomicU64,
    /// Frames the audio thread needed that were not ready.
    underruns: AtomicU64,
    /// The arrangement frame the audio side reads at (or moves to): which voices are due.
    playhead: AtomicU64,
    /// Each voice's jump ahead, in the voices' order.
    jumps: Vec<Jump>,
}

/// One voice on the worker side.
struct Writer {
    voice: StretchVoice,
    producer: Producer<f32>,
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
}

/// One voice on the audio side.
struct Reader {
    consumer: Consumer<f32>,
    start: u64,
    length: u64,
    /// The clip frame at the head of the ring.
    next: u64,
    /// Rewound, and nothing has arrived since: the silence until the worker catches up is the
    /// jump's, not an underrun.
    rewound: bool,
    /// The jump ahead asked for and not yet drained, if any.
    jumping: Option<u64>,
    /// Its track is heard (not muted, not left out by a solo): a start waits for it.
    heard: bool,
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
            self.shared.paused.store(requested, Ordering::Release);
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
            for (index, writer) in self.writers.iter_mut().enumerate() {
                if writer.start > pass || writer.start.saturating_add(writer.length) < playhead {
                    continue;
                }
                let Some(jump) = self.shared.jumps.get(index) else { continue };
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
        for (writer, jump) in self.writers.iter_mut().zip(&self.shared.jumps) {
            let local = to.saturating_sub(writer.start).min(writer.length);
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

/// A thread running [`StretchWorker`]s, one per graph (ADR 0005): a worker joins through
/// [`StretchThread::add`] and leaves once its graph has been dropped. Dropping the handle stops
/// the thread.
pub(crate) struct StretchThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    inbox: Producer<StretchWorker>,
}

impl StretchThread {
    pub(crate) fn spawn() -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let (inbox, mut incoming) = RingBuffer::<StretchWorker>::new(INBOX);
        let handle = std::thread::Builder::new().name("suhina-stretch".into()).spawn(move || {
            let mut workers: Vec<StretchWorker> = Vec::new();
            while !stop_flag.load(Ordering::Relaxed) {
                while let Ok(worker) = incoming.pop() {
                    workers.push(worker);
                }
                // A worker whose reader (its graph) is gone has nobody to render for.
                workers.retain(|w| Arc::strong_count(&w.shared) > 1);
                let mut did = false;
                for worker in &mut workers {
                    did |= worker.work();
                }
                if !did {
                    std::thread::sleep(IDLE);
                }
            }
        })?;
        Ok(Self { stop, handle: Some(handle), inbox })
    }

    /// Hands a graph's worker to the thread.
    pub(crate) fn add(&mut self, worker: StretchWorker) -> Result<(), String> {
        self.inbox.push(worker).map_err(|_| "the stretch worker thread is not taking new work".to_string())
    }
}

impl Drop for StretchThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // A worker that panicked has nothing left to clean up.
            let _ = handle.join();
        }
    }
}

/// The audio thread's side: reads each voice's frames by their position in the clip.
pub(crate) struct AheadReader {
    readers: Vec<Reader>,
    shared: Arc<Shared>,
    /// The worker, while no thread has taken it: the reader then fills the rings itself.
    inline: Option<StretchWorker>,
}

impl AheadReader {
    /// Splits `voices` (each with its clip's start and length in arrangement frames) into a reader
    /// and its worker, with a ring per voice holding [`AHEAD_SECONDS`] at `sample_rate`.
    pub(crate) fn new(voices: Vec<(StretchVoice, u64, u64)>, sample_rate: u32) -> Self {
        let frames = ((AHEAD_SECONDS * f64::from(sample_rate)) as usize).max(2 * BLOCK_FRAMES);
        let jumps = voices.iter().map(|_| Jump::default()).collect();
        let shared = Arc::new(Shared { jumps, ..Shared::default() });
        let mut readers = Vec::with_capacity(voices.len());
        let mut writers = Vec::with_capacity(voices.len());
        for (voice, start, length) in voices {
            let (producer, consumer) = RingBuffer::new(frames * 2);
            readers.push(Reader { consumer, start, length, next: 0, rewound: false, jumping: None, heard: true });
            writers.push(Writer { voice, producer, start, produced: 0, length, jumped: 0, seek_to: None });
        }
        let worker = StretchWorker {
            writers,
            shared: Arc::clone(&shared),
            generation: 0,
            scratch: vec![0.0; BLOCK_FRAMES * 2],
            ring: frames as u64,
        };
        Self { readers, shared, inline: Some(worker) }
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

    /// Asks for the voices to continue from arrangement frame `to`; until the worker has moved
    /// them, reads are silence.
    pub(crate) fn move_to(&mut self, to: u64) {
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
        self.readers.iter().filter(|r| r.heard).all(|reader| {
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

    /// Moves a pending rewind along; call it before reading. Never blocks.
    pub(crate) fn service(&mut self) {
        let requested = self.shared.requested.load(Ordering::Acquire);
        if self.shared.drained.load(Ordering::Acquire) == requested {
            return;
        }
        if let Some(worker) = self.inline.as_mut() {
            // Nothing else pushes: the reader stands in for the worker's side of the handshake.
            worker.shared.paused.store(requested, Ordering::Release);
        }
        if self.shared.paused.load(Ordering::Acquire) != requested {
            return;
        }
        let to = self.shared.seek_to.load(Ordering::Acquire);
        for (reader, jump) in self.readers.iter_mut().zip(&self.shared.jumps) {
            let queued = reader.consumer.slots();
            if let Ok(chunk) = reader.consumer.read_chunk(queued) {
                chunk.commit_all();
            }
            reader.next = to.saturating_sub(reader.start).min(reader.length);
            reader.rewound = true;
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
        out.fill(0.0);
        let wanted = (out.len() / 2) as u64;
        if let Some(reader) = self.readers.get(index) {
            self.shared.playhead.store(reader.start.saturating_add(from), Ordering::Release);
        }
        if let Some(worker) = self.inline.as_mut() {
            worker.fill();
        }
        let ready = self.shared.drained.load(Ordering::Acquire) == self.shared.requested.load(Ordering::Acquire);
        let Some(reader) = self.readers.get_mut(index) else { return };
        // A rewind under way, or done with nothing rendered since: silence the transport asked
        // for, not frames the worker failed to deliver.
        if !ready {
            return;
        }
        let Some(jump) = self.shared.jumps.get(index) else { return };
        // A jump ahead under way: once the worker has stopped pushing, the ring is emptied and
        // the voice goes on from where it jumped to.
        if let Some(requested) = reader.jumping {
            if jump.paused.load(Ordering::Acquire) != requested {
                self.shared.underruns.fetch_add(wanted, Ordering::Relaxed);
                return;
            }
            let queued = reader.consumer.slots();
            if let Ok(chunk) = reader.consumer.read_chunk(queued) {
                chunk.commit_all();
            }
            reader.next = jump.to.load(Ordering::Acquire).min(reader.length);
            reader.jumping = None;
            jump.drained.store(requested, Ordering::Release);
        }
        if reader.rewound {
            if reader.consumer.slots() == 0 {
                return;
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
            if reader.next < from && from - reader.next > BLOCK_FRAMES as u64 && reader.consumer.slots() == 0 {
                let requested = jump.requested.load(Ordering::Acquire) + 1;
                let to = from + wanted + JUMP_BLOCKS * BLOCK_FRAMES as u64;
                jump.to.store(to, Ordering::Release);
                jump.requested.store(requested, Ordering::Release);
                reader.jumping = Some(requested);
            }
            return;
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
        }
    }

    /// Frames needed that were not ready, since the renderer was built.
    pub(crate) fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }
}
