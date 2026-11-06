// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! (id, value) pairs an audio thread hands the main thread between blocks: what a plug-in
//! changed itself (its own editor, its internal mapping), or what the engine sent with the
//! blocks (a panel set, an automation lane). Written into fixed slots in turn — a burst past
//! the room loses the oldest slot's value, so what a parameter is at now is what survives —
//! and read out again whole, in the order the writes were made. The audio thread never locks,
//! waits or allocates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Most pairs one report holds: writes past it lose the oldest slot's value until that
/// parameter is written again. More parameter writes than this in the time between two
/// main-thread reads has not been seen; the values a plug-in runs with are re-read by the
/// listing every couple of seconds.
const SLOTS: usize = 256;

/// A slot with nothing written in it yet: not a parameter id (ids are 32-bit).
const FREE: u64 = u64::MAX;

/// One slot: a sequence number, odd while a write is under way, the pair it holds, and the
/// write's position in the cursor's count.
struct Slot {
    seq: AtomicUsize,
    id: AtomicU64,
    value: AtomicU64,
    pos: AtomicU64,
}

impl Slot {
    const fn new() -> Self {
        Self { seq: AtomicUsize::new(0), id: AtomicU64::new(FREE), value: AtomicU64::new(0), pos: AtomicU64::new(0) }
    }
}

/// What the audio thread noted since the main thread read it last. Made before processing
/// starts; the audio thread only writes, the main thread only reads.
pub(crate) struct ParamNotes {
    slots: Box<[Slot]>,
    cursor: AtomicU64,
}

/// A report shared between an audio thread and the main thread.
pub(crate) type Notes = Arc<ParamNotes>;

/// The main thread's memory of what each slot last said, one entry per slot: the write's
/// sequence that was reported from it, so [`ParamNotes::take`] reports a slot's change once,
/// not again on every read.
pub(crate) type Reads = Vec<Option<usize>>;

impl ParamNotes {
    pub(crate) fn new() -> Self {
        Self { slots: (0..SLOTS).map(|_| Slot::new()).collect(), cursor: AtomicU64::new(0) }
    }

    /// [`Reads`] as empty as its slots are: no slot has said anything yet.
    pub(crate) fn fresh_reads() -> Reads {
        vec![None; SLOTS]
    }

    /// Notes `value` as parameter `id`'s value now. Real-time safe: no lock, no allocation,
    /// it never waits.
    pub(crate) fn note(&self, id: u32, value: f64) {
        let pos = self.cursor.fetch_add(1, Ordering::Relaxed);
        let at = pos as usize % self.slots.len();
        let Some(slot) = self.slots.get(at) else { return };
        // The sequence marks the write: odd while it is under way, even when the pair holds
        // together, so a read either sees it whole or sees none of it.
        let _ = slot.seq.fetch_add(1, Ordering::AcqRel);
        slot.id.store(u64::from(id), Ordering::Relaxed);
        slot.value.store(value.to_bits(), Ordering::Relaxed);
        slot.pos.store(pos, Ordering::Relaxed);
        let _ = slot.seq.fetch_add(1, Ordering::AcqRel);
    }

    /// What has been noted since `read` last saw the slots, in the order the writes were
    /// made, each pair a slot's latest; `read` is left holding what was reported now. The
    /// writes the audio thread makes while the read runs wait for the next read, so what a
    /// read reports is in the order they were written. A slot still under a write is tried
    /// again, so a read sees a write whole or not at all. Never clears: what each slot said
    /// stays, so the audio thread can write over it without waiting for the reader.
    pub(crate) fn take(&self, read: &mut Reads) -> Vec<(u32, f64)> {
        self.take_until(self.cursor.load(Ordering::Relaxed), read)
    }

    /// [`ParamNotes::take`] up to the write at `until` (the first not delivered): the reads a
    /// caller makes while the audio thread writes.
    pub(crate) fn take_until(&self, until: u64, read: &mut Reads) -> Vec<(u32, f64)> {
        let start = until as usize % self.slots.len();
        let mut taken = Vec::new();
        for step in 0..self.slots.len() {
            let at = (start + step) % self.slots.len();
            let Some(slot) = self.slots.get(at) else { continue };
            loop {
                let before = slot.seq.load(Ordering::Acquire);
                if before % 2 == 1 {
                    continue;
                }
                let id = slot.id.load(Ordering::Relaxed);
                let value = slot.value.load(Ordering::Relaxed);
                let pos = slot.pos.load(Ordering::Relaxed);
                if slot.seq.load(Ordering::Acquire) != before {
                    continue;
                }
                // A write made while this read runs (or after it began) waits for the next
                // read, in its place in the order of the writes.
                if id != FREE && pos < until && read[at] != Some(before) {
                    taken.push((id as u32, f64::from_bits(value)));
                    read[at] = Some(before);
                }
                break;
            }
        }
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_noted_value_is_reported_once_and_a_change_again() {
        let notes = ParamNotes::new();
        let mut read = ParamNotes::fresh_reads();
        notes.note(5, 0.1);
        notes.note(7, 0.9);
        assert_eq!(notes.take(&mut read), [(5, 0.1), (7, 0.9)]);
        // Nothing changed: the same values are not reported again, and the reader of these
        // pairs takes what a later set says, not what a slot said once.
        assert!(notes.take(&mut read).is_empty());
        // The same value written again (through another slot) is reported again; the readers
        // of these pairs take it as the same value they already know.
        notes.note(5, 0.2);
        assert_eq!(notes.take(&mut read), [(5, 0.2)]);
    }

    #[test]
    fn a_burst_past_the_room_loses_the_oldest() {
        let notes = ParamNotes::new();
        let mut read = ParamNotes::fresh_reads();
        for id in 0..(SLOTS as u32 + 32) {
            notes.note(id, 0.5);
        }
        let taken = notes.take(&mut read);
        assert_eq!(taken.len(), SLOTS, "one slot each");
        assert_eq!(taken.first().map(|(id, _)| *id), Some(32), "the first 32 ids were written over");
        // The reads came in the order the writes were made.
        let ids: Vec<u32> = taken.iter().map(|(id, _)| *id).collect();
        let sorted: Vec<u32> = (32..(SLOTS as u32 + 32)).collect();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn the_reads_are_in_the_order_the_writes_were_made() {
        let notes = ParamNotes::new();
        let mut read = ParamNotes::fresh_reads();
        // Enough writes to wrap the slots, with one parameter written first and last: its
        // last value is what the read ends with.
        for id in 0..(SLOTS as u32) {
            notes.note(id, 0.5);
        }
        notes.note(5, 0.9);
        let taken = notes.take(&mut read);
        assert_eq!(taken.iter().rev().find(|(id, _)| *id == 5).map(|(_, value)| *value), Some(0.9));
    }

    /// A write of the same pair again after the slots wrapped round is still a write, and is
    /// reported again although its pair matches what the slot said before: the session's
    /// latest value for the parameter came from another slot in between.
    #[test]
    fn the_same_pair_written_again_after_a_wrapping_is_reported() {
        let notes = ParamNotes::new();
        let mut read = ParamNotes::fresh_reads();
        notes.note(5, 0.1);
        notes.note(5, 0.2);
        assert_eq!(notes.take(&mut read), [(5, 0.1), (5, 0.2)]);
        // Enough writes to bring the cursor round to the first slot again.
        for id in 0..(SLOTS as u32 - 2) {
            notes.note(id, 0.5);
        }
        notes.note(5, 0.1);
        let taken = notes.take(&mut read);
        assert_eq!(
            taken.len(),
            SLOTS - 1,
            "the 254 filler writes and the re-write; the slot whose pair was already read stays quiet"
        );
        assert_eq!(taken.last(), Some(&(5, 0.1)), "the newer write of the same pair, last of the reads");
    }

    /// A write made while a read is under way waits for the next read: what one read reports
    /// is in the order the writes were made, and nothing read is overtaken by what was read
    /// before it.
    #[test]
    fn a_write_made_while_a_read_runs_waits_for_the_next_read() {
        let notes = ParamNotes::new();
        let mut read = ParamNotes::fresh_reads();
        // Two writes of one parameter; the read is cut before the second: it reports the first
        // only, and the next read reports the second.
        notes.note(5, 0.1);
        notes.note(5, 0.2);
        assert_eq!(notes.take_until(1, &mut read), [(5, 0.1)]);
        assert_eq!(notes.take(&mut read), [(5, 0.2)]);
        // The same across the slots' wrap: a read cut between two writes keeps their order,
        // and what it left out comes after what it read, not before.
        notes.note(6, 0.3);
        notes.note(6, 0.4);
        assert_eq!(notes.take_until(3, &mut read), [(6, 0.3)]);
        assert_eq!(notes.take(&mut read), [(6, 0.4)]);
    }
}
