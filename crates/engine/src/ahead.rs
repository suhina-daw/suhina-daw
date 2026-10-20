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

use crate::stretch::StretchVoice;

/// Seconds of stretched audio each voice keeps ready.
const AHEAD_SECONDS: f64 = 0.5;
/// Project frames a voice renders per call.
const BLOCK_FRAMES: usize = 1024;
/// How long the worker thread sleeps when every ring is full.
const IDLE: Duration = Duration::from_millis(1);
/// How far past the playhead, in rings' lengths, voices are rendered ahead at all.
const HORIZON_RINGS: u64 = 4;
/// A voice's handover slot ([`Shared::adopted`]) the audio side closed at the swap: too late for
/// the worker to hand the voice over.
const CLOSED: u64 = u64::MAX;

/// The rings made so far, so each has its own number (a voice's handover names the ring).
static RINGS: AtomicU64 = AtomicU64::new(1);

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
    /// Each voice's handover from the graph before: 0 while open, the number of the ring the
    /// worker handed over to it, or [`CLOSED`] once the audio side swapped the graph in without
    /// one. Whichever side writes it first decides.
    adopted: Vec<AtomicU64>,
    /// Each voice's jump generation on the graph before when the worker handed it over: a jump
    /// that graph asked for after it is one the new writer did not follow.
    handed_jump: Vec<AtomicU64>,
}

/// One voice on the worker side.
struct Writer {
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
        for (index, writer) in self.writers.iter_mut().enumerate() {
            let Some(old_index) = by_key.remove(&writer.key) else { continue };
            let (Some(old), Some(old_jump), Some(slot)) =
                (before.writers.get_mut(old_index), before.shared.jumps.get(old_index), self.shared.adopted.get(index))
            else {
                continue;
            };
            if old_jump.requested.load(Ordering::Acquire) != old.jumped
                || (old.start, old.length) != (writer.start, writer.length)
            {
                continue;
            }
            // The jump the voice is at now, published with the claim below.
            if let Some(handed_jump) = self.shared.handed_jump.get(index) {
                handed_jump.store(old.jumped, Ordering::Release);
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
                while let Ok(mut worker) = incoming.pop() {
                    // The graph it replaces is the newest one here: its voices carry over.
                    if let Some(before) = workers.last_mut() {
                        worker.adopt_from(before);
                    }
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
    pub(crate) fn new(voices: Vec<(StretchVoice, u64, u64, u64)>, sample_rate: u32) -> Self {
        let frames = ((AHEAD_SECONDS * f64::from(sample_rate)) as usize).max(2 * BLOCK_FRAMES);
        let jumps = voices.iter().map(|_| Jump::default()).collect();
        let adopted = voices.iter().map(|_| AtomicU64::new(0)).collect();
        let handed_jump = voices.iter().map(|_| AtomicU64::new(0)).collect();
        let shared = Arc::new(Shared { jumps, adopted, handed_jump, ..Shared::default() });
        let mut readers = Vec::with_capacity(voices.len());
        let mut writers = Vec::with_capacity(voices.len());
        for (voice, start, length, key) in voices {
            let (producer, consumer) = RingBuffer::new(frames * 2);
            let ring = RINGS.fetch_add(1, Ordering::Relaxed);
            readers.push(Reader { consumer, ring, start, length, next: 0, rewound: false, jumping: None, heard: true });
            writers.push(Writer { voice, producer, key, ring, start, produced: 0, length, jumped: 0, seek_to: None });
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
    pub(crate) fn adopt_from(&mut self, before: &mut AheadReader) {
        for (index, (reader, slot)) in self.readers.iter_mut().zip(&self.shared.adopted).enumerate() {
            let ring = match slot.compare_exchange(0, CLOSED, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => continue,
                Err(ring) => ring,
            };
            let Some(old_index) = before.readers.iter().position(|r| r.ring == ring) else { continue };
            let Some(old) = before.readers.get_mut(old_index) else { continue };
            std::mem::swap(&mut reader.consumer, &mut old.consumer);
            std::mem::swap(&mut reader.ring, &mut old.ring);
            reader.next = old.next;
            reader.rewound = old.rewound;
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
    (region.warped, region.transpose.to_bits()).hash(&mut hasher);
    std::mem::discriminant(&region.stretch).hash(&mut hasher);
    for marker in &region.warp_markers {
        (marker.time.to_bits(), marker.audio_time.to_bits()).hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod handover_tests {
    use super::*;
    use suhina_model::{AudioRegion, Clip, ContentId, StretchMode, WarpMarker};

    const SR: u32 = 48_000;
    const LENGTH: u64 = 2 * SR as u64;

    /// A reader of one stretched voice: two seconds of a sine, warped to four beats at 100 bpm.
    fn reader() -> AheadReader {
        let samples = (0..2 * SR as usize).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let data = Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples });
        let region = AudioRegion {
            warped: true,
            stretch: StretchMode::Smooth,
            warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 4.0, audio_time: 2.0 }],
            sample_rate: Some(SR),
            channels: Some(1),
            duration_seconds: Some(2.0),
            ..AudioRegion::new("sine.wav")
        };
        let clip = Clip::new(ContentId(1), 0.0, 4.0);
        let Some(voice) = StretchVoice::new(&clip, &region, data, 100.0, SR) else { panic!("no voice") };
        AheadReader::new(vec![(voice, 0, LENGTH, 7)], SR)
    }

    /// The new graph's reader after its worker took the voice over and the swap, with a jump
    /// asked for on the old graph in between when `jump_between`.
    fn swapped(jump_between: bool) -> AheadReader {
        let (mut old, mut new) = (reader(), reader());
        let (Some(mut old_worker), Some(mut new_worker)) = (old.take_worker_unfilled(), new.take_worker_unfilled())
        else {
            panic!("no workers")
        };
        new_worker.adopt_from(&mut old_worker);
        assert_ne!(new.shared.adopted[0].load(Ordering::Acquire), 0, "handed over");
        if jump_between {
            // As `read` asks for one when the voice fell too far behind.
            old.shared.jumps[0].to.store(30_000, Ordering::Release);
            old.shared.jumps[0].requested.store(1, Ordering::Release);
            old.readers[0].jumping = Some(1);
        }
        new.adopt_from(&mut old);
        new
    }

    #[test]
    fn a_handed_over_voice_plays_on_from_where_it_was() {
        let new = swapped(false);
        assert_eq!(new.readers[0].jumping, None);
    }

    #[test]
    fn a_jump_the_old_graph_asked_for_after_the_handover_is_asked_for_again() {
        // The new writer renders the ring now and never saw the old graph's jump: the new
        // graph jumps the voice to the same frame, so the ring and the reader line up again.
        let new = swapped(true);
        assert_eq!(new.readers[0].jumping, Some(1));
        assert_eq!(new.shared.jumps[0].to.load(Ordering::Acquire), 30_000);
        assert_eq!(new.shared.jumps[0].requested.load(Ordering::Acquire), 1);
    }

    #[test]
    fn new_samples_under_the_same_pool_key_are_another_voice() {
        let region = AudioRegion::new("sine.wav");
        let clip = Clip::new(ContentId(1), 0.0, 4.0);
        let audio = || Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples: vec![0.0; 16] });
        let (a, b) = (audio(), audio());
        let key = |data: &Arc<crate::AudioData>| voice_key(&clip, &region, data, (0, LENGTH), 1);
        assert_eq!(key(&a), key(&a));
        assert_ne!(key(&a), key(&b), "the voice of the old samples is not taken over");
    }
}
