// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The output port worker's queue: a burst that fits the caps is kept whole and drained in
//! order, and a queue at a cap makes room with its oldest write — the newest beats the oldest.

use super::*;

/// Every write the queue holds, in order, after it is closed — closing lets the drain end, so
/// the test never waits on a clock. The queue drains what it holds first.
fn drained(queue: &OutputQueue) -> Vec<Vec<u8>> {
    queue.close();
    let mut writes = Vec::new();
    while let Some(bytes) = queue.take() {
        writes.push(bytes);
    }
    writes
}

#[test]
fn a_burst_that_fits_is_kept_whole_in_order() {
    let queue = OutputQueue::default();
    for n in 0..OUTPUT_QUEUE {
        queue.push((n as u32).to_le_bytes().to_vec());
    }
    let writes = drained(&queue);
    assert_eq!(writes.len(), OUTPUT_QUEUE, "every write of the burst is kept");
    for (n, bytes) in writes.iter().enumerate() {
        assert_eq!(bytes, &(n as u32).to_le_bytes().to_vec(), "write {n} in order");
    }
}

#[test]
fn a_queue_at_its_count_cap_makes_room_with_the_oldest() {
    let queue = OutputQueue::default();
    for n in 0..=OUTPUT_QUEUE {
        queue.push((n as u32).to_le_bytes().to_vec());
    }
    let writes = drained(&queue);
    assert_eq!(writes.len(), OUTPUT_QUEUE, "still the cap's count");
    assert_eq!(writes[0], (1u32).to_le_bytes().to_vec(), "the oldest went");
    assert_eq!(writes[OUTPUT_QUEUE - 1], (OUTPUT_QUEUE as u32).to_le_bytes().to_vec(), "the newest came in");
}

#[test]
fn a_queue_at_its_byte_cap_makes_room_with_the_oldest() {
    const CHUNK: usize = 1024 * 1024;
    let queue = OutputQueue::default();
    for n in 0..=OUTPUT_QUEUE_BYTES / CHUNK {
        queue.push(vec![n as u8; CHUNK]);
    }
    let writes = drained(&queue);
    let kept: Vec<Vec<u8>> = (1..=OUTPUT_QUEUE_BYTES / CHUNK).map(|n| vec![n as u8; CHUNK]).collect();
    assert_eq!(writes, kept, "the oldest went and the rest stay in order");
}

#[test]
fn a_write_the_whole_budget_goes_when_the_queue_is_empty() {
    let queue = OutputQueue::default();
    queue.push(vec![7; OUTPUT_QUEUE_BYTES]);
    let writes = drained(&queue);
    assert_eq!(writes, vec![vec![7; OUTPUT_QUEUE_BYTES]], "a write within the budget is never refused");
}
