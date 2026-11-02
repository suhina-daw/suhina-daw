// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::*;

const BLOCK: usize = 256;
const LONG: Duration = Duration::from_secs(5);

/// A host on a thread that halves every sample, and on block `slow` stays until it is told to go.
fn halving_host(
    address: String,
    blocks: usize,
    slow: Option<(usize, mpsc::Receiver<()>)>,
) -> thread::JoinHandle<usize> {
    thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut count = 0;
        while count < blocks {
            let hold = slow.as_ref().filter(|(at, _)| *at == count).map(|(_, go)| go);
            if served.serve(LONG, |input, output| {
                if let Some(go) = hold {
                    let _ = go.recv();
                }
                for (o, i) in output.iter_mut().zip(input) {
                    *o = i * 0.5;
                }
            }) {
                count += 1;
            }
        }
        count
    })
}

fn ramp(len: usize, offset: f32) -> Vec<f32> {
    (0..len).map(|n| n as f32 + offset).collect()
}

#[test]
fn blocks_go_there_and_back() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let host = halving_host(link.address(), 100, None);
    let mut output = vec![0.0; BLOCK * 2];
    for block in 0..100 {
        let input = ramp(BLOCK * 2, block as f32);
        assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done, "block {block}");
        assert!(output.iter().zip(&input).all(|(o, i)| *o == i * 0.5), "block {block}");
    }
    assert_eq!(host.join().ok(), Some(100));
}

#[test]
fn shorter_blocks_and_odd_lengths() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let host = halving_host(link.address(), 2, None);
    // 7 samples are 3 whole frames; the rest of the output stays as it was.
    let mut output = vec![9.0; 7];
    assert_eq!(link.process(&ramp(7, 1.0), &mut output, LONG), Outcome::Done);
    assert_eq!(output, [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 9.0]);
    // Longer than the link's block: only the first BLOCK frames go.
    let mut output = vec![9.0; BLOCK * 4];
    assert_eq!(link.process(&ramp(BLOCK * 4, 0.0), &mut output, LONG), Outcome::Done);
    assert_eq!(output[BLOCK * 2 - 1], (BLOCK * 2 - 1) as f32 * 0.5);
    assert_eq!(output[BLOCK * 2], 9.0);
    assert_eq!(host.join().ok(), Some(2));
}

#[test]
fn a_slow_host_misses_then_the_link_is_busy_until_it_answers() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let (go, hold) = mpsc::channel();
    let host = halving_host(link.address(), 3, Some((1, hold)));
    let input = ramp(BLOCK * 2, 1.0);
    let mut output = vec![0.0; BLOCK * 2];
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);

    let timeout = Duration::from_millis(10);
    output.fill(7.0);
    assert_eq!(link.process(&input, &mut output, timeout), Outcome::Missed);
    assert!(output.iter().all(|&o| o == 7.0), "a missed block leaves the output alone");
    assert_eq!(link.process(&input, &mut output, timeout), Outcome::Busy);

    // The link stays busy until the slow host answers.
    assert!(go.send(()).is_ok());
    assert!(link.wait_ready(Instant::now() + LONG));
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);
    assert_eq!(output[3], input[3] * 0.5);
    assert_eq!(host.join().ok(), Some(3));
}

#[test]
fn without_a_host_every_block_misses_or_is_busy() {
    let mut link = Link::create(1, 64).unwrap_or_else(|e| panic!("{e}"));
    let mut output = [0.0; 64];
    let timeout = Duration::from_millis(5);
    assert_eq!(link.process(&[1.0; 64], &mut output, timeout), Outcome::Missed);
    for _ in 0..100 {
        assert_eq!(link.process(&[1.0; 64], &mut output, timeout), Outcome::Busy);
    }
}

#[test]
fn a_host_waiting_for_blocks_gives_up_at_its_timeout() {
    let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let mut served = Served::open(&link.address()).unwrap_or_else(|e| panic!("{e}"));
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    assert!(!served.serve(Duration::from_millis(5), move |_, _| flag.store(true, Ordering::Relaxed)));
    assert!(!called.load(Ordering::Relaxed));
    assert_eq!(served.channels(), 2);
}

#[test]
fn sizes_are_checked() {
    for (channels, frames) in [(0, 64), (2, 0), (MAX_CHANNELS + 1, 64), (2, MAX_FRAMES + 1)] {
        assert!(matches!(Link::create(channels, frames), Err(IpcError::BadSize { .. })), "{channels} {frames}");
    }
    let link = Link::create(MAX_CHANNELS, 16).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((link.channels(), link.max_frames()), (MAX_CHANNELS, 16));
}

#[test]
fn opening_something_else_fails_cleanly() {
    let stranger = std::env::temp_dir().join(format!("not-a-link-{}", std::process::id()));
    let _ = std::fs::write(&stranger, [0u8; 1024]);
    assert!(matches!(Served::open(&stranger.display().to_string()), Err(IpcError::BadLink(_))));
    let _ = std::fs::remove_file(&stranger);

    // Shared memory of a link's kind that does not hold one.
    let short = shm::with_bytes("ipc", &[0u8; 16]);
    assert!(matches!(Served::open(short.address()), Err(IpcError::BadLink(_))));
    let garbage = shm::with_bytes("ipc", &[0xAAu8; 4096]);
    assert!(matches!(Served::open(garbage.address()), Err(IpcError::BadLink(_))));
    // An audio source is not a link, whatever it holds.
    let source = AudioSource::from_interleaved(&[0.5; 4096], 2, 48_000.0).unwrap_or_else(|e| panic!("{e}"));
    assert!(Served::open(&source.address()).is_err());

    let own = std::process::id();
    for address in [
        "suhina-ipc-does-not-exist".to_string(),
        "Local\\suhina-ipc-does-not-exist".to_string(),
        "Local\\suhina-ipc-1-2-3".to_string(),
        format!("/proc/{own}/fd/0"),
        format!("/proc/{own}/fd/99999"),
        format!("/proc/{own}/fd/0#0"),
        format!("/proc/{own}/fd/0#"),
        "/proc/self/fd/0".to_string(),
        String::new(),
    ] {
        assert!(Served::open(&address).is_err(), "{address}");
    }
}

#[test]
fn the_region_goes_with_the_link() {
    let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    let file = cfg!(not(any(windows, target_os = "linux")));
    // On Linux the address is the engine's descriptor under /proc, which exists while it holds
    // it: no file in /dev/shm. On Windows it is a mapping's name, no file at all.
    if cfg!(target_os = "linux") {
        let path = address.split('#').next().unwrap_or_default();
        assert!(address.starts_with("/proc/") && std::path::Path::new(path).exists(), "{address}");
    } else {
        assert_eq!(std::path::Path::new(&address).exists(), file, "{address}");
    }
    drop(link);
    // On Linux the address is a descriptor number, which another test may take next.
    if !cfg!(target_os = "linux") {
        assert!(Served::open(&address).is_err());
    }
    if file {
        assert!(!std::path::Path::new(&address).exists());
    }
}

/// A descriptor number that is closed and taken by another region must not open as the old one
/// (a host that opens late, after the engine gave up on its load).
#[test]
#[cfg(target_os = "linux")]
fn a_reused_descriptor_number_does_not_open_the_old_address() {
    let first = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let old = first.address();
    let number = old.split("/fd/").nth(1).and_then(|r| r.split('#').next()).map(str::to_string);
    drop(first);
    // Make links until one has the number the first had (usually the very next).
    let mut keep = Vec::new();
    for _ in 0..64 {
        let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
        let same = link.address().split("/fd/").nth(1).and_then(|r| r.split('#').next()).map(str::to_string) == number;
        keep.push(link);
        if same {
            break;
        }
    }
    assert!(matches!(Served::open(&old), Err(IpcError::BadLink(_))), "{old}");
    for link in &keep {
        assert!(Served::open(&link.address()).is_ok());
    }
}

#[test]
fn a_link_works_on_once_its_name_is_gone() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    let host = halving_host(address.clone(), 2, None);
    // Wait until the host has opened it, as the engine waits for the host's answer to a load.
    let mut output = vec![0.0; BLOCK * 2];
    assert_eq!(link.process(&ramp(BLOCK * 2, 0.0), &mut output, LONG), Outcome::Done);
    link.host_opened();
    if cfg!(not(any(windows, target_os = "linux"))) {
        assert!(!std::path::Path::new(&address).exists());
    }
    assert_eq!(link.process(&ramp(BLOCK * 2, 1.0), &mut output, LONG), Outcome::Done);
    assert_eq!(output, ramp(BLOCK * 2, 1.0).iter().map(|v| v * 0.5).collect::<Vec<_>>());
    assert_eq!(host.join().unwrap_or_else(|_| panic!("host")), 2);
}

#[test]
fn events_go_with_their_block() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    // A host that writes, for each note-on, its key at the event's frame (left channel).
    let host = thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut seen = Vec::new();
        while seen.len() < 3 {
            served.serve_with_events(LONG, |_, events, output| {
                for event in events {
                    if let EventKind::NoteOn { key, .. } = event.kind
                        && let Some(sample) = output.get_mut(event.frame as usize * 2)
                    {
                        *sample = f32::from(key);
                    }
                }
                seen.push(events.len());
            });
        }
        seen
    });
    let input = vec![0.0; BLOCK * 2];
    let mut output = vec![0.0; BLOCK * 2];
    let events = [Event::note_on(0, 0, 60, 1.0), Event::note_on(10, 0, 64, 0.5), Event::note_off(20, 0, 60, 0.0)];
    assert_eq!(link.process_with_events(&input, &events, &mut output, LONG), Outcome::Done);
    assert_eq!((output[0], output[20], output[40]), (60.0, 64.0, 0.0));
    // The next block has none; one past the block's end lands on its last frame.
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);
    assert!(output.iter().all(|s| *s == 0.0));
    let late = [Event::note_on(5_000, 0, 72, 1.0)];
    assert_eq!(link.process_with_events(&input[..20], &late, &mut output[..20], LONG), Outcome::Done);
    assert_eq!(output[18], 72.0);
    assert_eq!(host.join().ok(), Some(vec![3, 0, 1]));

    // Past MAX_EVENTS, the rest are dropped.
    let many = vec![Event::note_on(0, 0, 1, 1.0); MAX_EVENTS + 5];
    let mut link = Link::create(1, 16).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    let host = thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut count = 0;
        while !served.serve_with_events(LONG, |_, events, _| count = events.len()) {}
        count
    });
    assert_eq!(link.process_with_events(&[0.0; 16], &many, &mut [0.0; 16], LONG), Outcome::Done);
    assert_eq!(host.join().ok(), Some(MAX_EVENTS));
}

#[test]
fn a_block_names_the_instances_that_run_it_after_the_links_own() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    // A host that records each block's followers and adds 1 per instance run (its own first).
    let host = thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut seen = Vec::new();
        while seen.len() < 3 {
            served.serve_joint(LONG, |input, _, followers, output| {
                let runs = 1.0 + followers.len() as f32;
                for (o, i) in output.iter_mut().zip(input) {
                    *o = *i + runs;
                }
                seen.push(followers.to_vec());
            });
        }
        seen
    });
    let input = vec![1.0; BLOCK * 2];
    let mut output = vec![0.0; BLOCK * 2];
    assert!(link.ready());
    assert!(link.send_joint(&input, &[], &[7, 3, 9], output.len()));
    assert!(!link.ready(), "a block is out");
    assert_eq!(link.wait(&mut output, Instant::now() + LONG), Outcome::Done);
    assert!(output.iter().all(|s| *s == 5.0));
    assert!(link.ready());
    // A plain block names none; past MAX_FOLLOWERS the rest are dropped.
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);
    let many: Vec<u32> = (0..MAX_FOLLOWERS as u32 + 10).collect();
    assert!(link.send_joint(&input, &[], &many, output.len()));
    assert_eq!(link.wait(&mut output, Instant::now() + LONG), Outcome::Done);
    let seen = host.join().unwrap_or_else(|_| panic!("host"));
    assert_eq!(seen[0], [7, 3, 9]);
    assert!(seen[1].is_empty());
    assert_eq!(seen[2], many[..MAX_FOLLOWERS]);
}

#[test]
fn looking_for_an_answer_leaves_the_block_to_wait() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    // Nothing out: nothing to wait for.
    assert!(link.wait_answered(Instant::now()));
    let (go, hold) = mpsc::channel();
    let host = halving_host(link.address(), 2, Some((0, hold)));
    let input = ramp(BLOCK * 2, 1.0);
    let mut output = vec![0.0; BLOCK * 2];
    assert!(link.send_with_events(&input, &[], output.len()));
    // A look that is already past its time finds nothing, and a short wait gives up.
    assert!(!link.wait_answered(Instant::now()));
    assert!(!link.wait_answered(Instant::now() + Duration::from_millis(5)));
    // Neither took the block: it is still out, and its answer comes.
    assert!(!link.ready());
    assert!(go.send(()).is_ok());
    assert!(link.wait_answered(Instant::now() + LONG));
    // Answered, but its result is not taken until `wait`, which then does not wait.
    assert!(!link.ready());
    assert_eq!(link.wait(&mut output, Instant::now()), Outcome::Done);
    assert_eq!(output[3], input[3] * 0.5);
    assert!(link.ready());
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);
    assert_eq!(host.join().ok(), Some(2));
}
