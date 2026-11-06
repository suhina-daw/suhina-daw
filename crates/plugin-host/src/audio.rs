// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running instances' blocks on audio threads, one instance after another when a block asks
//! for it (ADR 0006, decision 2).
//!
//! Every instance has a link and an audio thread of its own, which serves the link's blocks
//! with the instance's [`Block`]. A block may name other instances of this process to run after
//! the link's own ([`suhina_ipc::Served::serve_joint`]): the thread that has the block then
//! runs each of them in turn, in place, and answers once, so consecutive plug-ins of a chain
//! cost the engine one round trip a block instead of one each.
//!
//! So that any audio thread can run an instance, each instance's [`Block`] sits in a lock of its
//! own while its thread serves, and the process keeps them by instance number. A thread holds
//! one lock at a time (its own block's, then each follower's), so two threads that run each
//! other's instances never wait on each other. An instance's own thread does not touch its block
//! while another runs it: the engine sends a plug-in's block either on its own link or as a
//! follower on another's, never both at once.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use suhina_ipc::{Event, Served};

/// How long an audio thread waits for a block before checking whether it should stop.
const POLL: Duration = Duration::from_millis(100);

/// One instance's audio processing, which any audio thread of the process may run, one at a
/// time.
pub(crate) trait Block: Send {
    /// Runs one block: `output` (zeroed, as long as `input`) gets the result of `input`
    /// (interleaved) with `events` at their frames.
    fn process(&mut self, input: &[f32], events: &[Event], output: &mut [f32]);
}

/// How the link's channels meet a plug-in's main buses, the same in every format (#767): a
/// plug-in channel the link has no channel for repeats the link's last one, so a mono source
/// reaches both sides of a stereo input; a link channel the plug-in has no output channel for
/// repeats the output's last, so a mono output reaches both sides of the link.
///
/// `input` is the link's interleaved block of `link_channels` channels; the main input bus's
/// channels `bus` get its first `frames` frames. Does nothing without link channels.
pub(crate) fn feed_main<B: AsMut<[f32]>>(input: &[f32], link_channels: usize, frames: usize, bus: &mut [B]) {
    if link_channels == 0 {
        return;
    }
    for (index, channel) in bus.iter_mut().enumerate() {
        let source = index.min(link_channels - 1);
        for (frame, sample) in channel.as_mut().iter_mut().take(frames).enumerate() {
            *sample = input.get(frame * link_channels + source).copied().unwrap_or(0.0);
        }
    }
}

/// The main output bus's channels `bus` into the link's interleaved `output` of `link_channels`
/// channels, `frames` frames (see [`feed_main`]). An empty bus leaves `output` as it is.
pub(crate) fn collect_main<B: AsRef<[f32]>>(bus: &[B], link_channels: usize, frames: usize, output: &mut [f32]) {
    let Some(last) = bus.len().checked_sub(1) else { return };
    for channel in 0..link_channels {
        let Some(source) = bus.get(channel.min(last)) else { continue };
        for (frame, sample) in source.as_ref().iter().take(frames).enumerate() {
            if let Some(out) = output.get_mut(frame * link_channels + channel) {
                *out = *sample;
            }
        }
    }
}

/// An instance's block behind its lock, as the other audio threads see it.
trait Member: Send + Sync {
    /// Runs `input` through the instance into `output`; one that is no longer running passes
    /// the sound through, as a plug-in that misses its block does.
    fn run(&self, input: &[f32], events: &[Event], output: &mut [f32]);
}

impl<B: Block> Member for Mutex<Option<B>> {
    fn run(&self, input: &[f32], events: &[Event], output: &mut [f32]) {
        match lock(self).as_mut() {
            Some(block) => {
                output.fill(0.0);
                block.process(input, events, output);
            }
            None => {
                let len = output.len().min(input.len());
                if let (Some(to), Some(from)) = (output.get_mut(..len), input.get(..len)) {
                    to.copy_from_slice(from);
                }
            }
        }
    }
}

/// Locks `mutex`, taking it over from a thread that panicked while holding it.
fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The instances whose audio threads serve now, by number.
static MEMBERS: Mutex<BTreeMap<u32, Arc<dyn Member>>> = Mutex::new(BTreeMap::new());

fn member(instance: u32) -> Option<Arc<dyn Member>> {
    lock(&MEMBERS).get(&instance).cloned()
}

/// Whether `instance`'s audio thread serves blocks (and can be run after others).
pub(crate) fn serving(instance: u32) -> bool {
    lock(&MEMBERS).contains_key(&instance)
}

/// Waits up to `wait` for `instance`'s audio thread to serve, so that a block that names it as
/// a follower right after it loaded finds it; whether it does.
pub(crate) fn wait_serving(instance: u32, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        if serving(instance) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Serves `served` on the calling thread, `instance`'s audio thread, until `stop` is set: each
/// block runs through `block`, then through the instances it names, in order. Meanwhile other
/// instances' blocks can run this one after theirs. Returns `block` (`None` only if it was
/// lost, which does not happen).
pub(crate) fn serve<B: Block + 'static>(instance: u32, block: B, served: &mut Served, stop: &AtomicBool) -> Option<B> {
    let own: Arc<Mutex<Option<B>>> = Arc::new(Mutex::new(Some(block)));
    let shared: Arc<dyn Member> = own.clone();
    lock(&MEMBERS).insert(instance, Arc::clone(&shared));
    let samples = served.channels() as usize * served.max_frames() as usize;
    let mut between = vec![0.0f32; samples];
    while !stop.load(Ordering::Acquire) {
        served.serve_joint(POLL, |input, events, followers, output| {
            shared.run(input, events, output);
            for &follower in followers {
                // An instance that is gone passes the sound through.
                let (Some(next), Some(between)) = (member(follower), between.get_mut(..output.len())) else {
                    continue;
                };
                between.copy_from_slice(output);
                next.run(between, events, output);
            }
        });
    }
    {
        let mut members = lock(&MEMBERS);
        if members.get(&instance).is_some_and(|m| std::ptr::addr_eq(Arc::as_ptr(m), Arc::as_ptr(&shared))) {
            members.remove(&instance);
        }
    }
    lock(&own).take()
}

#[cfg(test)]
mod tests {
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
        feed_main::<Vec<f32>>(&stereo, 2, 2, &mut []);
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
        collect_main::<Vec<f32>>(&[], 2, 2, &mut kept);
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
}
