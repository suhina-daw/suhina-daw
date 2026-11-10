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
//! that finds too little is filled with silence and counted. Nothing here allocates, locks or
//! frees on the render thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::Consumer;

use crate::chain::MAX_BLOCK;

/// The most input channels the render thread reads; the output layer writes no more.
pub const MAX_INPUT_CHANNELS: usize = 64;
/// Whole render calls of input that may wait in the ring; past that the oldest are dropped
/// down to two calls, once before rendering, independently of internal chunk boundaries.
const MAX_BACKLOG_BLOCKS: usize = 4;

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
        }
    }

    /// Reads from `ring`, `channels` samples per frame, from now on; blocks that find too little
    /// count in `underruns`. Channels past [`MAX_INPUT_CHANNELS`] are not taken: the writer
    /// writes `channels.min(MAX_INPUT_CHANNELS)` per frame.
    pub(crate) fn attach(&mut self, ring: Consumer<f32>, channels: usize, underruns: Arc<AtomicU64>) {
        self.ring = Some(ring);
        self.channels = channels.clamp(1, MAX_INPUT_CHANNELS);
        self.underruns = underruns;
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
                self.diagnostics.backlog_drops.fetch_add(1, Ordering::Relaxed);
                self.diagnostics.backlog_frames.fetch_add(dropped_frames as u64, Ordering::Relaxed);
            }
        }
    }

    /// Takes `frames` frames of input for this block (at most [`MAX_BLOCK`]): what the ring has,
    /// then silence. Without an input device, silence.
    pub(crate) fn read(&mut self, frames: usize) {
        let frames = frames.min(MAX_BLOCK);
        self.frames = frames;
        let channels = self.channels;
        let wanted = frames * channels;
        let Some(block) = self.scratch.get_mut(..wanted) else { return };
        let Some(ring) = self.ring.as_mut() else {
            block.fill(0.0);
            return;
        };
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
            if let Some(rest) = block.get_mut(take..) {
                rest.fill(0.0);
            }
            self.diagnostics.short_read(take / channels, frames);
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Frames that waited in the ring when this block's were read, them included: how long
    /// ago, beyond the input device's own latency, this block's first frame came in. 0 without
    /// an input device.
    pub(crate) fn waiting(&self) -> u64 {
        if self.ring.is_some() { self.waiting as u64 } else { 0 }
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
