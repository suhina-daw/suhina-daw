// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mix tap (`docs/spec/mix-space.md`): every metered track's sound, block by block, for the
//! mix space analysis on another thread.
//!
//! The audio thread copies each track's block into one lock-free queue only while a reader has
//! asked for it ([`MixTap::set_points`]), so nothing is copied while no Mix Space device is open.
//! A block that does not fit is dropped and counted; the audio thread never waits. Two points can
//! be tapped: after the track's devices and before its volume and pan (pre-fader), and after them
//! (post-fader, what the level meters show).
//!
//! A block in the queue is two header values, the point and slot packed into the bits of the
//! first and the frame count in the second, then the interleaved stereo frames.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

/// Samples the queue holds: about two seconds of a hundred stereo tracks at 48 kHz is far more
/// than a reader polling every few milliseconds needs.
const CAPACITY: usize = 1 << 21;

/// Where in a track's chain its sound is tapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TapPoint {
    /// After the track's devices, before its volume and pan.
    PreFader,
    /// After its volume and pan: what its level meter shows.
    PostFader,
}

impl TapPoint {
    fn bit(self) -> u32 {
        match self {
            Self::PreFader => 1 << 31,
            Self::PostFader => 0,
        }
    }
}

/// The shared half of the tap, inside the [`crate::MeterMonitor`]: which points are wanted, the
/// reader's end of the queue until a reader takes it, and how many blocks were dropped.
#[derive(Default)]
pub struct MixTap {
    pre: AtomicBool,
    post: AtomicBool,
    sample_rate: AtomicU32,
    dropped: AtomicU64,
    reader: Mutex<Option<Consumer<f32>>>,
}

impl std::fmt::Debug for MixTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixTap")
            .field("pre", &self.pre.load(Ordering::Relaxed))
            .field("post", &self.post.load(Ordering::Relaxed))
            .field("dropped", &self.dropped.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl MixTap {
    /// Asks the audio thread to copy the points that are true, from the next block on.
    pub fn set_points(&self, pre_fader: bool, post_fader: bool) {
        self.pre.store(pre_fader, Ordering::Relaxed);
        self.post.store(post_fader, Ordering::Relaxed);
    }

    /// Whether `point` is being copied.
    pub fn is_tapping(&self, point: TapPoint) -> bool {
        match point {
            TapPoint::PreFader => self.pre.load(Ordering::Relaxed),
            TapPoint::PostFader => self.post.load(Ordering::Relaxed),
        }
    }

    /// The sample rate of the blocks.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Blocks dropped because the reader was behind.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The reader's end of the queue, once: a second call returns `None` until the renderer is
    /// built again. Never call it from the audio thread.
    pub fn take_reader(&self) -> Option<MixTapReader> {
        let consumer = self.reader.lock().ok()?.take()?;
        Some(MixTapReader { consumer, block: Vec::new() })
    }

    fn give(&self, consumer: Consumer<f32>, sample_rate: u32) {
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
        if let Ok(mut reader) = self.reader.lock() {
            *reader = Some(consumer);
        }
    }
}

/// The reader's end of the tap. See [`MixTap::take_reader`].
pub struct MixTapReader {
    consumer: Consumer<f32>,
    block: Vec<f32>,
}

impl MixTapReader {
    /// Calls `each` with every block in the queue, oldest first: its point, the track's meter
    /// slot and its interleaved stereo frames. Returns how many blocks were read.
    pub fn read(&mut self, mut each: impl FnMut(TapPoint, usize, &[f32])) -> usize {
        let mut count = 0;
        loop {
            let (tag, frames) = {
                let Ok(header) = self.consumer.read_chunk(2) else { break };
                let (first, second) = header.as_slices();
                let mut values = first.iter().chain(second);
                let (Some(tag), Some(frames)) = (values.next(), values.next()) else { break };
                (tag.to_bits(), frames.to_bits() as usize)
            };
            // The writer commits a whole block at once, so the rest is there.
            let Ok(chunk) = self.consumer.read_chunk(2 + frames * 2) else { break };
            let (first, second) = chunk.as_slices();
            self.block.clear();
            self.block.extend(first.iter().chain(second).skip(2));
            chunk.commit_all();
            let point = if tag & (1 << 31) != 0 { TapPoint::PreFader } else { TapPoint::PostFader };
            each(point, (tag & !(1 << 31)) as usize, &self.block);
            count += 1;
        }
        count
    }
}

/// The audio thread's end of the tap, owned by the renderer's meters.
pub(crate) struct MixTapWriter {
    producer: Producer<f32>,
}

impl MixTapWriter {
    /// Makes the queue and hands its reader's end to `tap`. Allocates: call it off the audio
    /// thread.
    pub(crate) fn new(tap: &MixTap, sample_rate: u32) -> Self {
        let (producer, consumer) = RingBuffer::new(CAPACITY);
        tap.give(consumer, sample_rate);
        Self { producer }
    }

    /// Copies `block` (interleaved stereo) for `slot` at `point`, or drops it and counts it when
    /// the queue is too full. Never allocates, locks or waits.
    pub(crate) fn write(&mut self, tap: &MixTap, point: TapPoint, slot: usize, block: &[f32]) {
        let Ok(slot) = u32::try_from(slot) else { return };
        if slot >= 1 << 31 {
            return;
        }
        let frames = block.len() / 2;
        let Ok(count) = u32::try_from(frames) else { return };
        let Ok(mut chunk) = self.producer.write_chunk(2 + frames * 2) else {
            tap.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let (first, second) = chunk.as_mut_slices();
        let header = [f32::from_bits(point.bit() | slot), f32::from_bits(count)];
        let source = header.iter().chain(block.iter().take(frames * 2));
        for (to, from) in first.iter_mut().chain(second.iter_mut()).zip(source) {
            *to = *from;
        }
        chunk.commit_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_arrive_in_order_with_their_point_and_slot() {
        let tap = MixTap::default();
        let mut writer = MixTapWriter::new(&tap, 48_000);
        writer.write(&tap, TapPoint::PostFader, 3, &[0.1, 0.2, 0.3, 0.4]);
        writer.write(&tap, TapPoint::PreFader, 511, &[0.5, 0.6]);
        let mut reader = tap.take_reader().expect("reader");
        assert!(tap.take_reader().is_none(), "only one reader");
        let mut seen = Vec::new();
        let count = reader.read(|point, slot, block| seen.push((point, slot, block.to_vec())));
        assert_eq!(count, 2);
        assert_eq!(seen[0], (TapPoint::PostFader, 3, vec![0.1, 0.2, 0.3, 0.4]));
        assert_eq!(seen[1], (TapPoint::PreFader, 511, vec![0.5, 0.6]));
        assert_eq!(reader.read(|_, _, _| {}), 0);
        assert_eq!(tap.sample_rate(), 48_000);
    }

    #[test]
    fn a_full_queue_drops_blocks_instead_of_waiting() {
        let tap = MixTap::default();
        let mut writer = MixTapWriter::new(&tap, 48_000);
        let block = vec![0.25; 4096];
        let fits = CAPACITY / (block.len() + 2);
        for _ in 0..fits + 10 {
            writer.write(&tap, TapPoint::PostFader, 0, &block);
        }
        assert_eq!(tap.dropped(), 10);
        let mut reader = tap.take_reader().expect("reader");
        assert_eq!(reader.read(|_, _, b| assert_eq!(b.len(), 4096)), fits);
    }
}
