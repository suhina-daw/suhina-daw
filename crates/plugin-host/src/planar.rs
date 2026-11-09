// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What every format runs a block on: its audio buffers ([`Planar`]) and the parameter values
//! waiting for it ([`ParamQueue`]), made before processing starts so an audio thread never
//! allocates or waits (#764 › 10).

use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

/// A block's audio in one direction: buses × channels × the most frames a block has, each
/// channel its own run of samples. Allocated once and never resized, so the channel pointers it
/// hands the formats that pass pointer arrays ([`Planar::pointers`]) stay valid as long as it
/// lives, also when the `Planar` itself moves (into a block, onto an audio thread): they point at
/// its heap memory, which does not move with it.
#[derive(Default)]
pub(crate) struct Planar {
    /// Every channel of every bus, `frames` samples each, one after another.
    samples: Vec<f32>,
    frames: usize,
    /// Each bus's channels, as indices into the channels of all buses.
    buses: Vec<Range<usize>>,
    /// One pointer per channel, at the channel's first sample, in the order of `buses`.
    pointers: Vec<*mut f32>,
}

// SAFETY: the pointers point into the `Planar`'s own samples and are written through only by
// whoever holds it mutably, so it can move to another thread as its samples could.
unsafe impl Send for Planar {}

impl Planar {
    /// Zeroed buffers of `frames` samples for each bus's channels, `buses` giving how many
    /// channels each bus has. The callers cap the counts a plug-in gives.
    pub(crate) fn new(buses: impl IntoIterator<Item = usize>, frames: usize) -> Self {
        let mut next = 0usize;
        let buses: Vec<Range<usize>> = buses
            .into_iter()
            .map(|channels| {
                let start = next;
                next = start.saturating_add(channels);
                start..next
            })
            .collect();
        let mut samples = vec![0.0f32; next.saturating_mul(frames)];
        let base = samples.as_mut_ptr();
        // `wrapping_add` stays within `samples` (channel × frames is at most its length) without
        // `unsafe`; the pointers are only read through by the plug-in, within that length.
        let pointers = (0..next).map(|channel| base.wrapping_add(channel * frames)).collect();
        Self { samples, frames, buses, pointers }
    }

    /// How many buses it has.
    pub(crate) fn bus_count(&self) -> usize {
        self.buses.len()
    }

    /// How many channels bus `bus` has (none for a bus it does not have).
    pub(crate) fn channels(&self, bus: usize) -> usize {
        self.buses.get(bus).map_or(0, Range::len)
    }

    /// Where bus `bus`'s channels are in `samples` (empty for a bus it does not have): the one
    /// place that knows the layout.
    fn sample_range(&self, bus: usize) -> Range<usize> {
        self.buses.get(bus).map_or(0..0, |r| r.start * self.frames..r.end * self.frames)
    }

    /// Bus `bus`'s samples, every channel's run one after another (empty for a bus it does not
    /// have).
    fn samples_of(&self, bus: usize) -> &[f32] {
        self.samples.get(self.sample_range(bus)).unwrap_or_default()
    }

    /// Bus `bus`'s channels, each its full length.
    pub(crate) fn bus(&self, bus: usize) -> std::slice::ChunksExact<'_, f32> {
        self.samples_of(bus).chunks_exact(self.frames.max(1))
    }

    /// Bus `bus`'s channels to write, each its full length.
    pub(crate) fn bus_mut(&mut self, bus: usize) -> std::slice::ChunksExactMut<'_, f32> {
        let range = self.sample_range(bus);
        self.samples.get_mut(range).unwrap_or_default().chunks_exact_mut(self.frames.max(1))
    }

    /// Every channel of every bus.
    pub(crate) fn buses_mut(&mut self) -> impl ExactSizeIterator<Item = std::slice::ChunksExactMut<'_, f32>> {
        let frames = self.frames;
        let mut rest = self.samples.as_mut_slice();
        self.buses.iter().map(move |range| {
            let taken = std::mem::take(&mut rest);
            let (bus, after) = taken.split_at_mut((range.len() * frames).min(taken.len()));
            rest = after;
            bus.chunks_exact_mut(frames.max(1))
        })
    }

    /// Zeroes the first `frames` samples of every channel of bus `bus`.
    pub(crate) fn silence(&mut self, bus: usize, frames: usize) {
        for channel in self.bus_mut(bus) {
            channel.iter_mut().take(frames).for_each(|sample| *sample = 0.0);
        }
    }

    /// Bus `bus`'s channel pointers, as `AudioBusBuffers` and VST2's `processReplacing` take
    /// them: an array of [`Planar::channels`] pointers, each at `frames` samples. Valid while
    /// the `Planar` lives; nothing else may hold its samples while a plug-in writes through
    /// them. A bus it does not have gives an empty array.
    pub(crate) fn pointers(&mut self, bus: usize) -> *mut *mut f32 {
        let range = self.buses.get(bus).cloned().unwrap_or(0..0);
        self.pointers.get_mut(range).unwrap_or_default().as_mut_ptr()
    }
}

/// Parameter values waiting for an instance's next block: parameter id and value (CLAP's plain
/// value, VST3's normalized one), in the order they were set. The main thread pushes; the audio
/// thread takes with [`ParamQueue::take`], which never waits for the lock and never allocates.
/// Cloning gives another handle on the same queue.
#[derive(Clone, Default)]
pub(crate) struct ParamQueue(Arc<Mutex<Vec<(u32, f64)>>>);

impl ParamQueue {
    /// Adds a value for the next block. Main thread only: it waits for the lock.
    pub(crate) fn push(&self, id: u32, value: f64) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push((id, value));
    }

    /// Sets the value `id` gets with the next block, in place of any still waiting for it.
    pub(crate) fn replace(&self, id: u32, value: f64) {
        let mut queue = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        queue.retain(|(other, _)| *other != id);
        queue.push((id, value));
    }

    /// Adds several values for the next block, in order. Main thread only.
    pub(crate) fn extend(&self, values: impl IntoIterator<Item = (u32, f64)>) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).extend(values);
    }

    /// Whether no value is waiting. Waits for the lock: not for an audio thread.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).is_empty()
    }

    /// Gives `each` the oldest values waiting, at most `limit` of them, in the order they were
    /// pushed, and leaves the rest for the next block. Real-time safe: when the main thread holds
    /// the lock it takes nothing and the values wait, and it neither allocates nor frees.
    pub(crate) fn take(&self, limit: usize, mut each: impl FnMut(u32, f64)) {
        let Ok(mut queue) = self.0.try_lock() else { return };
        let room = queue.len().min(limit);
        for (id, value) in queue.drain(..room) {
            each(id, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_planar_has_each_bus_and_channel_at_full_length() {
        let mut planar = Planar::new([2, 0, 3], 4);
        assert_eq!(planar.bus_count(), 3);
        assert_eq!((planar.channels(0), planar.channels(1), planar.channels(2), planar.channels(3)), (2, 0, 3, 0));
        assert_eq!(planar.bus(2).len(), 3);
        assert!(planar.bus(2).all(|channel| channel == [0.0; 4]));
        for (index, channel) in planar.bus_mut(2).enumerate() {
            channel.fill(index as f32 + 1.0);
        }
        assert!(planar.bus(0).all(|channel| channel == [0.0; 4]), "other buses are left alone");
        let last: Vec<&[f32]> = planar.bus(2).collect();
        assert_eq!(last, [[1.0; 4], [2.0; 4], [3.0; 4]]);
        let shapes: Vec<usize> = planar.buses_mut().map(|bus| bus.len()).collect();
        assert_eq!(shapes, [2, 0, 3]);
        planar.silence(2, 2);
        assert_eq!(planar.bus(2).next(), Some(&[0.0, 0.0, 1.0, 1.0][..]));
        assert_eq!(planar.bus(9).len(), 0);
    }

    #[test]
    fn its_pointers_stay_valid_when_it_moves() {
        let mut planar = Planar::new([1, 2], 3);
        let pointers = planar.pointers(1);
        // Moved into a box and back out, as a block moves onto its audio thread.
        let mut moved = *Box::new(planar);
        let channels: Vec<*mut f32> = (0..2)
            .map(|index| {
                // SAFETY: the array has as many pointers as bus 1 has channels.
                unsafe { *pointers.wrapping_add(index) }
            })
            .collect();
        for (index, pointer) in channels.into_iter().enumerate() {
            for frame in 0..3 {
                // SAFETY: each pointer is at a channel of 3 samples the moved `Planar` still owns,
                // and nothing else holds them now, as when a plug-in writes its output.
                unsafe { *pointer.wrapping_add(frame) = (index * 10 + frame) as f32 };
            }
        }
        assert_eq!(moved.pointers(1), pointers, "the array itself did not move");
        let written: Vec<&[f32]> = moved.bus(1).collect();
        assert_eq!(written, [[0.0, 1.0, 2.0], [10.0, 11.0, 12.0]]);
        assert!(moved.bus(0).all(|channel| channel == [0.0; 3]));
    }

    #[test]
    fn values_come_out_in_order_and_past_the_limit_wait_for_the_next_block() {
        let queue = ParamQueue::default();
        queue.push(3, 0.3);
        queue.extend([(1, 0.1), (2, 0.2)]);
        queue.push(3, 0.4);
        assert!(!queue.is_empty());
        let mut taken = Vec::new();
        queue.clone().take(3, |id, value| taken.push((id, value)));
        assert_eq!(taken, [(3, 0.3), (1, 0.1), (2, 0.2)]);
        taken.clear();
        queue.take(3, |id, value| taken.push((id, value)));
        assert_eq!(taken, [(3, 0.4)], "the rest came with the next block");
        queue.take(3, |id, value| taken.push((id, value)));
        assert_eq!(taken, [(3, 0.4)], "nothing is left");
        assert!(queue.is_empty());
    }

    #[test]
    fn replacing_drops_the_value_still_waiting_for_that_parameter() {
        let queue = ParamQueue::default();
        queue.push(1, 0.1);
        queue.push(2, 0.2);
        queue.replace(1, 0.9);
        let mut taken = Vec::new();
        queue.take(8, |id, value| taken.push((id, value)));
        assert_eq!(taken, [(2, 0.2), (1, 0.9)]);
    }

    #[test]
    fn a_take_while_the_main_thread_holds_the_lock_takes_nothing() {
        let queue = ParamQueue::default();
        queue.push(1, 0.5);
        let held = queue.0.lock().unwrap();
        let mut taken = 0;
        queue.take(8, |_, _| taken += 1);
        assert_eq!(taken, 0);
        drop(held);
        queue.take(8, |_, _| taken += 1);
        assert_eq!(taken, 1, "the value waited for the next block");
    }
}
