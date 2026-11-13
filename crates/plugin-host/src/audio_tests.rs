// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_ipc::{Link, Outcome};

/// Adds `step` to every sample and counts its blocks.
struct Add {
    step: f32,
    blocks: Arc<std::sync::atomic::AtomicU32>,
}

impl Block for Add {
    fn process(&mut self, input: &[f32], _events: &[Event], output: &mut [f32]) {
        self.blocks.fetch_add(1, Ordering::Relaxed);
        for (o, i) in output.iter_mut().zip(input) {
            *o = (*i + self.step) * 2.0;
        }
    }
}

/// An instance serving its own link on a thread of its own.
fn start(
    instance: u32,
    step: f32,
) -> (Link, Arc<AtomicBool>, Arc<std::sync::atomic::AtomicU32>, std::thread::JoinHandle<bool>) {
    let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let mut served = Served::open(&link.address()).unwrap_or_else(|e| panic!("{e}"));
    let stop = Arc::new(AtomicBool::new(false));
    let blocks = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let (thread_stop, block) = (stop.clone(), Add { step, blocks: blocks.clone() });
    let thread = std::thread::spawn(move || serve(instance, block, &mut served, &thread_stop).is_some());
    assert!(wait_serving(instance, Duration::from_secs(5)));
    (link, stop, blocks, thread)
}

/// Half the smallest normal `f32` in every sample: subnormal unless the thread flushes it to
/// zero. With `clears`, it then turns flushing off, as a plug-in that sets a mode of its own
/// does. Counts its blocks.
struct Subnormal {
    clears: bool,
    blocks: Arc<std::sync::atomic::AtomicU32>,
}

impl Block for Subnormal {
    fn process(&mut self, _input: &[f32], _events: &[Event], output: &mut [f32]) {
        self.blocks.fetch_add(1, Ordering::Relaxed);
        output.fill(std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5));
        if self.clears {
            crate::denormals::clear();
        }
    }
}

/// A `Subnormal` instance serving its own link on a thread of its own, its block count, and
/// what stops it. Its first block comes once the thread serves, so nothing waits here.
fn subnormal(
    instance: u32,
    clears: bool,
) -> (Link, Arc<AtomicBool>, Arc<std::sync::atomic::AtomicU32>, std::thread::JoinHandle<bool>) {
    let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let mut served = Served::open(&link.address()).unwrap_or_else(|e| panic!("{e}"));
    let stop = Arc::new(AtomicBool::new(false));
    let blocks = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let (thread_stop, block) = (stop.clone(), Subnormal { clears, blocks: blocks.clone() });
    let thread = std::thread::spawn(move || serve(instance, block, &mut served, &thread_stop).is_some());
    (link, stop, blocks, thread)
}

/// Sends one block on `link` and waits for its answer (the deadline is an hour away: only the
/// answer ends the wait in practice).
fn round_trip(link: &mut Link, followers: &[u32]) -> [f32; 8] {
    let mut output = [1.0f32; 8];
    assert!(link.send_joint(&[1.0f32; 8], &[], followers, output.len()));
    assert_eq!(link.wait(&mut output, Instant::now() + Duration::from_secs(3600)), Outcome::Done);
    output
}

/// #1258: a plug-in's block runs with subnormal numbers flushed to zero, also after a plug-in
/// turned flushing off, in its own next block and in a follower it runs before.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn blocks_run_with_subnormals_flushed_even_after_a_plug_in_turns_it_off() {
    let flushed = |output: [f32; 8]| output.iter().all(|s| s.to_bits() == 0);
    let (mut first, stop_first, _, first_thread) = subnormal(9_011, true);
    let (mut second, stop_second, second_blocks, second_thread) = subnormal(9_012, false);
    // Its first block, and the next one, which comes after it turned flushing off.
    for _ in 0..2 {
        let output = round_trip(&mut first, &[]);
        assert!(flushed(output), "{output:?}");
    }
    // The second answers a block of its own, so its thread serves it and it can follow.
    assert!(flushed(round_trip(&mut second, &[])));
    // Run after the first, which turns flushing off, on the first one's thread.
    let output = round_trip(&mut first, &[9_012]);
    assert!(flushed(output), "{output:?}");
    assert_eq!(second_blocks.load(Ordering::Relaxed), 2, "the second ran as a follower");
    for (stop, thread) in [(stop_first, first_thread), (stop_second, second_thread)] {
        stop.store(true, Ordering::Release);
        assert_eq!(thread.join().ok(), Some(true));
    }
}

#[test]
fn a_mono_link_reaches_every_channel_of_a_stereo_input() {
    let mono = [1.0f32, 2.0, 3.0];
    let mut bus = vec![vec![9.0f32; 4]; 2];
    feed_main(&mono, 1, 3, &mut bus);
    assert_eq!(bus[0][..3], [1.0, 2.0, 3.0]);
    assert_eq!(bus[1][..3], [1.0, 2.0, 3.0], "the right input repeats the link's last channel");
    assert_eq!(bus[1][3], 9.0, "past the block nothing changes");
}

#[test]
fn a_stereo_link_feeds_each_channel_and_repeats_its_last_into_the_rest() {
    let stereo = [1.0f32, -1.0, 2.0, -2.0];
    let mut bus = vec![vec![0.0f32; 2]; 4];
    feed_main(&stereo, 2, 2, &mut bus);
    assert_eq!(bus, vec![vec![1.0, 2.0], vec![-1.0, -2.0], vec![-1.0, -2.0], vec![-1.0, -2.0]]);
    // Fewer plug-in channels than the link's: the first ones.
    let mut mono = vec![vec![0.0f32; 2]];
    feed_main(&stereo, 2, 2, &mut mono);
    assert_eq!(mono, vec![vec![1.0, 2.0]]);
    // No link channels, no input bus, nothing to read: nothing happens, and nothing panics.
    feed_main(&stereo, 0, 2, &mut mono);
    feed_main::<&mut Vec<f32>>(&stereo, 2, 2, &mut []);
    feed_main(&[], 2, 2, &mut mono);
    assert_eq!(mono, vec![vec![0.0, 0.0]]);
}

#[test]
fn a_mono_output_reaches_both_sides_of_a_stereo_link() {
    let mut output = vec![0.0f32; 6];
    collect_main(&[vec![1.0f32, 2.0, 3.0]], 2, 3, &mut output);
    assert_eq!(output, [1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
}

#[test]
fn a_wider_output_gives_the_link_its_first_channels_and_a_stereo_one_each_its_own() {
    let bus = vec![vec![1.0f32, 2.0], vec![-1.0, -2.0], vec![5.0, 6.0]];
    let mut stereo = vec![0.0f32; 4];
    collect_main(&bus, 2, 2, &mut stereo);
    assert_eq!(stereo, [1.0, -1.0, 2.0, -2.0]);
    // A link with more channels than the output repeats the output's last.
    let mut wide = vec![0.0f32; 8];
    collect_main(&bus[..2], 4, 2, &mut wide);
    assert_eq!(wide, [1.0, -1.0, -1.0, -1.0, 2.0, -2.0, -2.0, -2.0]);
    // No output bus leaves the link's block as it was.
    let mut kept = vec![7.0f32; 4];
    collect_main::<&Vec<f32>>(&[], 2, 2, &mut kept);
    assert_eq!(kept, [7.0; 4]);
}

#[test]
fn a_block_runs_its_followers_on_the_same_thread_in_order() {
    let (mut first, stop_first, first_blocks, first_thread) = start(9_001, 1.0);
    let (_second, stop_second, second_blocks, second_thread) = start(9_002, 10.0);
    let (_third, stop_third, _, third_thread) = start(9_003, 100.0);
    let input = [1.0f32; 8];
    let mut output = [0.0f32; 8];
    let long = Instant::now() + Duration::from_secs(5);
    // The first, then the third, then the second; an unknown instance passes the sound.
    assert!(first.send_joint(&input, &[], &[9_003, 77, 9_002], output.len()));
    assert_eq!(first.wait(&mut output, long), Outcome::Done);
    let expected = ((((1.0 + 1.0) * 2.0) + 100.0) * 2.0 + 10.0) * 2.0;
    assert!(output.iter().all(|s| *s == expected), "{output:?}");
    assert_eq!(first_blocks.load(Ordering::Relaxed), 1);
    assert_eq!(second_blocks.load(Ordering::Relaxed), 1);

    // A stopped instance is no longer run; the sound passes it.
    stop_second.store(true, Ordering::Release);
    assert_eq!(second_thread.join().ok(), Some(true));
    assert!(!serving(9_002));
    assert!(first.send_joint(&input, &[], &[9_002], output.len()));
    assert_eq!(first.wait(&mut output, long), Outcome::Done);
    assert!(output.iter().all(|s| *s == 4.0), "{output:?}");

    for (stop, thread) in [(stop_first, first_thread), (stop_third, third_thread)] {
        stop.store(true, Ordering::Release);
        assert_eq!(thread.join().ok(), Some(true));
    }
}
