// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio input ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)): what the
//! input device (microphones, a guitar, an interface's line inputs) hears, read by the render
//! thread and heard through the tracks that take it.
//!
//! The output layer opens the input device and writes its frames, every channel interleaved,
//! into a lock-free ring ([`Renderer::set_audio_input`](crate::Renderer::set_audio_input)). At
//! the start of each block the render thread takes one block of frames from it into a scratch
//! buffer made up front; each track with an audio input then adds its channel (or pair) to its
//! chain or to the output, the way an audio clip does, while its monitoring says so. The input
//! device runs on its own clock: input that piles up past a few blocks is dropped, and a block
//! that finds too little holds the last sample each channel delivered and is counted — a
//! splice to silence cracked through everything downstream, and a recording baked hundreds of
//! those splices into its takes ([#1290] › the takes' crackle); a starvation that sustains
//! fades its hold toward zero, so a dead input never becomes a DC offset; and the frames a
//! hold made up are skipped when the device delivers them late, so the rest of a take keeps
//! the beat. Nothing here allocates, locks or frees on the render thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::Consumer;

use crate::chain::MAX_BLOCK;

/// The most input channels the render thread reads; the output layer writes no more.
pub const MAX_INPUT_CHANNELS: usize = 64;
/// Whole render calls of input that may wait in the ring; past that the oldest are dropped
/// down to two calls, once before rendering, independently of internal chunk boundaries.
const MAX_BACKLOG_BLOCKS: usize = 4;
/// The hold's level after one more block of sustained starvation: a block at 1024 frames and
/// 48 kHz is 21 ms, so the hold is gone after about a second of a dead input, and the crackle
/// fix's first held block still carries the signal's last state whole.
pub(crate) const HOLD_DECAY: f32 = 0.9;

/// A bounded diagnostic history, published without logging on the render thread.
#[derive(Default)]
pub struct InputDiagnostics {
    events: [AtomicU64; 32],
    sequence: AtomicU64,
    backlog_drops: AtomicU64,
    backlog_frames: AtomicU64,
}

/// Input diagnostics read on a control thread. Counters are independent snapshots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputSnapshot {
    /// Latest 32 short reads, ordered by sequence: [sequence, available, wanted] frames.
    /// Sequence numbers wrap after 2^32 events; each row is one coherent atomic load.
    pub short_reads: Vec<[u64; 3]>,
    /// Successful removals of old input from the ring.
    pub backlog_drops: u64,
    /// Frames removed by those drops.
    pub backlog_frames: u64,
}

impl InputDiagnostics {
    fn short_read(&self, available: usize, wanted: usize) {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        // Both sizes are capped at MAX_BLOCK (1024). Pack a complete event so a reader
        // cannot combine one underrun's available frames with another's requested frames.
        let event =
            ((sequence & 0xffff_ffff) << 32) | ((available.min(MAX_BLOCK) as u64) << 16) | wanted.min(MAX_BLOCK) as u64;
        if let Some(slot) = self.events.get((sequence % 32) as usize) {
            slot.store(event, Ordering::Relaxed);
        }
    }

    /// Read on the UI/control thread only: allocates the bounded event list there.
    pub fn snapshot(&self) -> InputSnapshot {
        let mut short_reads: Vec<_> = self
            .events
            .iter()
            .map(|slot| slot.load(Ordering::Relaxed))
            .filter(|event| *event != 0)
            .map(|event| [event >> 32, (event >> 16) & 0xffff, event & 0xffff])
            .collect();
        short_reads.sort_unstable_by_key(|event| event[0]);
        InputSnapshot {
            short_reads,
            backlog_drops: self.backlog_drops.load(Ordering::Relaxed),
            backlog_frames: self.backlog_frames.load(Ordering::Relaxed),
        }
    }
}

/// The render thread's end of the input device's ring.
pub(crate) struct Input {
    ring: Option<Consumer<f32>>,
    pub(crate) diagnostics: Arc<InputDiagnostics>,
    /// Samples per frame in the ring, 1 to [`MAX_INPUT_CHANNELS`].
    channels: usize,
    /// Blocks that found less input than they needed.
    underruns: Arc<AtomicU64>,
    /// This block's input, `channels` samples per frame.
    scratch: Vec<f32>,
    /// Frames in `scratch`.
    frames: usize,
    /// Frames that waited in the ring when this block's were read, them included.
    waiting: usize,
    /// The last sample each channel delivered, for a starved block's hold: the block holds
    /// the signal's last state instead of dropping to silence — a silence splice cracks
    /// through everything downstream, and a recording baked hundreds of them in.
    last: [f32; MAX_INPUT_CHANNELS],
    /// The hold's level at this block's start, a fraction of `last`: 1.0 until the starvation
    /// sustains, then decaying toward zero, so an input that never returns (an interface
    /// unplugged mid-session, a driver that stopped calling back) does not hold a DC offset
    /// through monitoring, the chain and the take for as long as the transport runs.
    hold: f32,
    /// The previous block starved: its hold is under way, and this one's fades on from it.
    starved: bool,
    /// Frames of input time covered since the renderer started: every block's frames, and the
    /// frames dropped when the ring overflowed. The input's own clock: its frames arrive in
    /// order whatever the render thread's position does, so a recording's positions run on it
    /// and never jump the way a position against the render's clock jumps with the ring's
    /// depth.
    consumed: u64,
    /// Frames a starved block's hold made up, that the device may still deliver late rather
    /// than dropping them: they stay in the ring while every later block reads its own
    /// `frames`, so the rest of a take runs that count late against the beat. A read that
    /// finds the ring a whole block deeper than the last whole read left it skips them —
    /// their window already spent its input time, and the hold is the content the take
    /// carries for it.
    owed: usize,
    /// The ring's leftover frames after the last read that took a whole block: the steady
    /// excess a delivery's uneven pace leaves and a backlog drop keeps. A late-held delivery
    /// arrives as a jump over this steady depth, which is what the skip reads it against —
    /// a steady depth itself (drift, the drop's kept block) is never a late delivery.
    excess: usize,
}

impl Input {
    pub(crate) fn new() -> Self {
        Self {
            ring: None,
            diagnostics: Arc::new(InputDiagnostics::default()),
            channels: 1,
            underruns: Arc::new(AtomicU64::new(0)),
            scratch: vec![0.0; MAX_INPUT_CHANNELS * MAX_BLOCK],
            frames: 0,
            waiting: 0,
            last: [0.0; MAX_INPUT_CHANNELS],
            hold: 1.0,
            starved: false,
            consumed: 0,
            owed: 0,
            excess: 0,
        }
    }

    /// Reads from `ring`, `channels` samples per frame, from now on; blocks that find too little
    /// count in `underruns`. Channels past [`MAX_INPUT_CHANNELS`] are not taken: the writer
    /// writes `channels.min(MAX_INPUT_CHANNELS)` per frame. The ring's own history starts here:
    /// what a past ring's starvation owed or its reads left over is not this ring's.
    pub(crate) fn attach(&mut self, ring: Consumer<f32>, channels: usize, underruns: Arc<AtomicU64>) {
        self.ring = Some(ring);
        self.channels = channels.clamp(1, MAX_INPUT_CHANNELS);
        self.underruns = underruns;
        self.owed = 0;
        self.excess = 0;
    }

    /// Discards stale input once before a whole render call. A loop, graph swap or MAX_BLOCK
    /// split must not shrink this budget midway through consuming the call's input.
    pub(crate) fn begin_block(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        let Some(ring) = self.ring.as_mut() else { return };
        let available = ring.slots() / self.channels;
        if available > frames.saturating_mul(MAX_BACKLOG_BLOCKS) {
            let dropped_frames = available.saturating_sub(frames.saturating_mul(2));
            // Bounded by ring.slots(), which contains only whole frames.
            let samples = dropped_frames * self.channels;
            if let Ok(chunk) = ring.read_chunk(samples) {
                chunk.commit_all();
                // Dropped frames were input time too: the take's positions jump over them, and
                // the writer fills their span with silence so the beat keeps.
                self.consumed += dropped_frames as u64;
                self.diagnostics.backlog_drops.fetch_add(1, Ordering::Relaxed);
                self.diagnostics.backlog_frames.fetch_add(dropped_frames as u64, Ordering::Relaxed);
            }
        }
    }

    /// Takes `frames` frames of input for this block (at most [`MAX_BLOCK`]): what the ring has,
    /// then the last state each channel delivered (a starved block holds the signal's last state
    /// instead of dropping to silence — a splice to silence and back cracks through everything
    /// downstream; a starvation that sustains fades its hold toward zero, so a dead input does
    /// not hold a DC offset forever; and a hold's frames the device delivers late are skipped,
    /// so the rest of a take keeps the beat). Without an input device, silence.
    pub(crate) fn read(&mut self, frames: usize) {
        let frames = frames.min(MAX_BLOCK);
        self.frames = frames;
        if frames == 0 {
            return;
        }
        // This block's window of input time is spent whatever the ring had: the input's own
        // clock runs at the render block's cadence.
        self.consumed += frames as u64;
        let channels = self.channels;
        let wanted = frames * channels;
        let Some(block) = self.scratch.get_mut(..wanted) else { return };
        let Some(ring) = self.ring.as_mut() else {
            block.fill(0.0);
            return;
        };
        // The held frames may still be delivered late rather than dropped (a device callback
        // that landed after its read): they stay in the ring while every later block reads its
        // own `frames`, so the ring carries them for good and the rest of a take runs that
        // count late against the beat. The read skips them when the ring carries a whole block
        // more than the last whole read left: the starvation emptied the ring, so the front
        // frames are the oldest thing in it — the late delivery — and the input time their
        // content belongs to was already spent (the starved block's window counted it, and the
        // hold is the content the take carries for it). A real loss leaves the ring at its own
        // pace, a block's production over the leftover a whole read leaves, and never that
        // much deeper — the skip fires on the jump a late delivery makes, not on a steady
        // depth (drift, the drop's kept block), and a loss is never mistaken for one.
        let depth = ring.slots() / channels;
        if self.owed > 0 && depth >= frames.saturating_add(self.owed).saturating_add(self.excess) {
            // Bounded by the depth the condition checked, which contains only whole frames.
            let samples = self.owed * channels;
            if let Ok(chunk) = ring.read_chunk(samples) {
                chunk.commit_all();
            }
            self.owed = 0;
        }
        self.waiting = ring.slots() / channels;
        let take = (ring.slots() / channels).min(frames) * channels;
        match ring.read_chunk(take) {
            Ok(chunk) => {
                let (first, second) = chunk.as_slices();
                for (slot, sample) in block.iter_mut().zip(first.iter().chain(second)) {
                    *slot = if sample.is_finite() { *sample } else { 0.0 };
                }
                chunk.commit_all();
            }
            Err(_) => block.fill(0.0),
        }
        if take < wanted {
            // The ring starved: the block holds the last state each channel delivered, never
            // dropping to silence — a splice to silence and back cracks through everything
            // downstream, and a recording baked hundreds of those splices into its takes
            // ([#1290] › the takes' crackle). The frames still count as the block's, so the
            // take's length and the recorder's timing stay exact. The held slots run from
            // `take` samples in; a slot's channel is its remainder (`take` is a sample count,
            // whole frames' worth).
            // The held state is the last frame the ring gave this block — the frame before the
            // held span — not the previous hold's own: a hold carried from before the fresh
            // samples would splice backward at the fresh-to-held boundary.
            if take > 0 {
                for channel in 0..channels {
                    if let Some(sample) = block.get((take / channels - 1) * channels + channel) {
                        self.last[channel] = *sample;
                    }
                }
            }
            // A hold carried past its first block fades toward zero ([#1290] › the review):
            // the first starved block holds the signal's last state whole (its splice in is
            // seamless, and the crackle fix stands), while an input that never returns — an
            // interface unplugged mid-session, a driver that stopped calling back — must not
            // hold a DC offset through monitoring, the chain and the take for as long as the
            // transport runs. Each further starved block fades from the level it starts at to
            // the decayed one, so the block boundaries stay continuous; a block with input
            // resets the hold to full strength, so the ring's own dips (which recover) hold
            // flat every time.
            let (start, end) = if self.starved { (self.hold, self.hold * HOLD_DECAY) } else { (self.hold, self.hold) };
            let span = (wanted - take) as f32;
            for (index, slot) in (take..wanted).enumerate() {
                let level = start + (end - start) * ((index + 1) as f32 / span);
                if let Some(sample) = block.get_mut(slot) {
                    *sample = self.last[slot % channels] * level;
                }
            }
            self.hold = end;
            self.starved = true;
            // The held frames are input time the block's window already spent: the device may
            // still deliver them late rather than dropping them, and a read that then finds
            // the ring a whole block deeper than a whole read leaves it skips them.
            self.owed += wanted - take;
            self.diagnostics.short_read(take / channels, frames);
            self.underruns.fetch_add(1, Ordering::Relaxed);
        } else {
            self.hold = 1.0;
            self.starved = false;
            // The leftover a whole read leaves is the steady depth a later late delivery jumps
            // over: a block's own production on top of what the pace and the backlog drop keep.
            self.excess = ring.slots() / channels;
            // The last state each channel delivered is the next starved block's hold: the
            // block's last frame, which the ring carried whole.
            for channel in 0..channels {
                if let Some(sample) = block.get((frames - 1) * channels + channel) {
                    self.last[channel] = *sample;
                }
            }
        }
    }

    /// Frames that waited in the ring when this block's were read, them included: how long
    /// ago, beyond the input device's own latency, this block's first frame came in. 0 without
    /// an input device.
    pub(crate) fn waiting(&self) -> u64 {
        if self.ring.is_some() { self.waiting as u64 } else { 0 }
    }

    /// The input's own clock: frames of input time covered since the renderer started — every
    /// block's frames and the ring's dropped ones. A recording's positions run on it, so they
    /// never jump the way a position against the render's clock jumps with the ring's depth.
    pub(crate) fn frames_consumed(&self) -> u64 {
        self.consumed
    }

    /// The sample of input channel `channel` (from 0) at `frame` of this block; silence for a
    /// channel the device does not have.
    pub(crate) fn sample(&self, frame: usize, channel: usize) -> f32 {
        if channel >= self.channels || frame >= self.frames {
            return 0.0;
        }
        self.scratch.get(frame * self.channels + channel).copied().unwrap_or(0.0)
    }
}

/// Where a track's audio input goes, prepared with the graph.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InputRoute {
    pub(crate) track: suhina_model::TrackId,
    /// Input channels, from 0, for the left and right side (the same for a mono input).
    pub(crate) left: usize,
    pub(crate) right: usize,
    pub(crate) monitor: suhina_model::Monitor,
    /// 0 for the output, or 1 + the index of the track's chain.
    pub(crate) bus: u32,
    pub(crate) gains: (f32, f32),
}
