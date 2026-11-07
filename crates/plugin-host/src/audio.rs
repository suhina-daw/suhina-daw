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
//!
//! Every audio thread runs its blocks with subnormal numbers flushed to zero, set again before each
//! instance's part, since a plug-in may change the mode while it runs ([`crate::denormals`]).

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
    let flush = crate::denormals::Flush::detect();
    flush.apply();
    while !stop.load(Ordering::Acquire) {
        served.serve_joint(POLL, |input, events, followers, output| {
            flush.apply();
            shared.run(input, events, output);
            for &follower in followers {
                // An instance that is gone passes the sound through.
                let (Some(next), Some(between)) = (member(follower), between.get_mut(..output.len())) else {
                    continue;
                };
                between.copy_from_slice(output);
                // The plug-in before it may have changed the mode.
                flush.apply();
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
#[path = "audio_tests.rs"]
mod tests;
