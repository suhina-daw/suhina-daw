// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine side against a real second process (`suhina-ipc-peer`): blocks round-trip, and
//! a peer that crashes or hangs in the middle of a block costs missed blocks, never a stall.
#![allow(clippy::panic)] // test helpers outside #[test] functions

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use suhina_ipc::{Link, Outcome, Served};

const FRAMES: usize = 128;
/// A block's budget at 128 frames and 48 kHz is 2.7 ms; the deadline for the peer is less.
const DEADLINE: Duration = Duration::from_millis(2);
/// Plenty, for blocks the peer is expected to answer (a cold process start, a busy CI machine).
const PATIENT: Duration = Duration::from_secs(5);

fn peer(link: &Link, mode: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_suhina-ipc-peer"))
        .args([link.address().as_str(), mode])
        .spawn()
        .unwrap_or_else(|e| panic!("starting the peer: {e}"))
}

fn input(block: usize) -> Vec<f32> {
    (0..FRAMES * 2).map(|n| (n + block) as f32).collect()
}

fn link() -> Link {
    Link::create(2, FRAMES as u32).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn round_trips_with_another_process() {
    let mut link = link();
    let mut child = peer(&link, "serve");
    let mut output = vec![0.0; FRAMES * 2];
    // The first block waits for the process to start.
    assert_eq!(link.process(&input(0), &mut output, PATIENT), Outcome::Done);

    let mut slowest = Duration::ZERO;
    let mut total = Duration::ZERO;
    let blocks = 2_000;
    for block in 1..=blocks {
        let started = Instant::now();
        assert_eq!(link.process(&input(block), &mut output, PATIENT), Outcome::Done, "block {block}");
        let took = started.elapsed();
        slowest = slowest.max(took);
        total += took;
        assert_eq!(output[5], input(block)[5] * 0.5);
    }
    eprintln!("round trip, back to back: mean {:?}, slowest {:?}", total / blocks as u32, slowest);

    // As in playback: the peer has gone to sleep by the time the next block comes.
    let (mut slowest, mut total) = (Duration::ZERO, Duration::ZERO);
    let blocks = 400;
    for block in 1..=blocks {
        std::thread::sleep(Duration::from_micros(2_500));
        let started = Instant::now();
        assert_eq!(link.process(&input(block), &mut output, PATIENT), Outcome::Done, "block {block}");
        let took = started.elapsed();
        slowest = slowest.max(took);
        total += took;
    }
    eprintln!("round trip, peer asleep: mean {:?}, slowest {:?}", total / blocks as u32, slowest);
    drop(link);
    let _ = child.wait();
}

/// Runs four good blocks, then checks that the failing peer costs only missed blocks.
fn survives(mode: &str) {
    let mut link = link();
    let mut child = peer(&link, mode);
    let mut output = vec![0.0; FRAMES * 2];
    for block in 0..4 {
        assert_eq!(link.process(&input(block), &mut output, PATIENT), Outcome::Done, "{mode} block {block}");
    }
    assert_eq!(link.process(&input(4), &mut output, DEADLINE), Outcome::Missed, "{mode}");
    for _ in 0..1_000 {
        assert_eq!(link.process(&input(5), &mut output, DEADLINE), Outcome::Busy, "{mode}");
    }
    let _ = child.kill();
    let _ = child.wait();
    // The failed peer leaves nothing behind: the region goes with the link. (On Linux the
    // address is a descriptor number, which another test may take next.)
    let address = link.address();
    drop(link);
    if !cfg!(target_os = "linux") {
        assert!(Served::open(&address).is_err(), "{mode}: {address}");
    }
}

#[test]
fn a_crashing_peer_costs_only_missed_blocks() {
    survives("crash");
}

#[test]
fn a_hanging_peer_costs_only_missed_blocks() {
    survives("hang");
}
