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
//! those splices into its takes ([#1290] › the takes' crackle). Nothing here allocates, locks
//! or frees on the render thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::Consumer;

use crate::chain::MAX_BLOCK;

/// The most input channels the render thread reads; the output layer writes no more.
pub const MAX_INPUT_CHANNELS: usize = 64;
/// Blocks of input that may wait in the ring; past that the oldest are dropped, down to two
/// blocks, so the input keeps up with the output when the two clocks drift apart.
const MAX_BACKLOG_BLOCKS: usize = 4;

/// The render thread's end of the input device's ring.
pub(crate) struct Input {
    ring: Option<Consumer<f32>>,
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
}

impl Input {
    pub(crate) fn new() -> Self {
        Self {
            ring: None,
            channels: 1,
            underruns: Arc::new(AtomicU64::new(0)),
            scratch: vec![0.0; MAX_INPUT_CHANNELS * MAX_BLOCK],
            frames: 0,
            waiting: 0,
            last: [0.0; MAX_INPUT_CHANNELS],
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
        let available = ring.slots() / channels;
        self.waiting = available.min(frames * MAX_BACKLOG_BLOCKS);
        if available > frames * MAX_BACKLOG_BLOCKS {
            let drop = (available - frames * 2) * channels;
            if let Ok(chunk) = ring.read_chunk(drop) {
                chunk.commit_all();
            }
        }
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
            // take's length and the recorder's timing stay exact.
            for frame in take..frames {
                for channel in 0..channels {
                    if let Some(slot) = block.get_mut(frame * channels + channel) {
                        *slot = self.last[channel];
                    }
                }
            }
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        // The block's last state is the next starved block's hold.
        for channel in 0..channels {
            if let Some(sample) = block.get((frames - 1) * channels + channel) {
                self.last[channel] = *sample;
            }
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
