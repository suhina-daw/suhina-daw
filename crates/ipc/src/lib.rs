// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio blocks between the engine and a plug-in host process (ADR 0006).
//!
//! The engine creates a [`Link`] (a shared-memory region and two doorbells) and starts the host
//! process with its [`Link::address`]; the host opens it with [`Served::open`]. Then, block by
//! block, [`Link::process`] copies the input into shared memory, rings the host and waits for
//! the output until a deadline; [`Served::serve`] waits for a block, lets the plug-ins fill the
//! output and rings back. A block can carry [`Event`]s (notes, controllers) at its frames, for
//! instruments: [`Link::process_with_events`] and [`Served::serve_with_events`].
//!
//! A host process runs several plug-in instances, each with its own link. A block can name
//! instances of the same process that run it after this link's own, in order, before the host
//! answers ([`Link::send_joint`], [`Served::serve_joint`]): consecutive plug-ins of a chain in
//! one process then cost one round trip a block instead of one each (ADR 0006, decision 2).
//!
//! One block is in flight at a time. If the host misses the deadline the engine gets
//! [`Outcome::Missed`], and until the host answers that block, [`Outcome::Busy`] at once,
//! without sending or waiting, so neither side touches the buffers while the other one owns
//! them. A host that crashes or hangs costs the engine the output of the blocks it misses,
//! never a crash or a wait past the deadline.
//!
//! This crate is one of the two allowed to use `unsafe` (ADR 0006): memory shared with another
//! process and waiting on it need it. Everything it exports is safe to use.
//!
//! In detail: Audio blocks between the engine and a plug-in host process (ADR 0006): a
//! shared-memory region with each block's notes and controllers (`Event`, up to 1024 a block) and
//! the instances of the host process that run the block after the link's own (up to 63,
//! `Link::send_joint`), one block in flight, a doorbell per direction (a futex on Linux, events
//! with a high-resolution timer on Windows, yielding elsewhere for now) and a deadline; a crashed
//! or hung host costs missed blocks, never a stall. `AudioSource`: a file's decoded samples in a
//! region of their own, written once and mapped read only by the host, for an ARA plug-in to read
//! (ADR 0033). Both are memory the operating system frees when the last process holding it ends,
//! however it ends (`shm`): a page-file mapping named `Local\suhina-…` on Windows, a `memfd` opened
//! through `/proc/<pid>/fd/<n>` on Linux, and on macOS a file in the temporary folder whose name
//! goes once the host has opened a link or the engine drops a source; the first one a process makes
//! removes the macOS files of processes that crashed before that, and earlier versions' leftover
//! files on every system (`sweep`, ADR 0006 › The boundary). One of the two crates allowed
//! `unsafe`. Linked into the engine's process through `plugins`.

mod doorbell;
mod events;
mod keyboard;
mod region;
mod shm;
mod source;
mod sweep;

pub use events::{Event, EventKind, MAX_EVENTS};
pub use keyboard::{keys_to_engine, pointer_on_screen, set_keys_to_engine, take_keyboard_back};
pub use source::{AudioSource, MAX_SOURCE_FRAMES};
pub use sweep::sweep_leftovers;

use std::fmt;
use std::time::{Duration, Instant};

use doorbell::Doorbell;
use region::Region;

/// Largest number of interleaved channels a link carries.
pub const MAX_CHANNELS: u32 = 64;
/// Largest block, in frames.
pub const MAX_FRAMES: u32 = 65_536;
/// Most instances a block names to run after the link's own ([`Link::send_joint`]).
pub const MAX_FOLLOWERS: usize = 63;

#[derive(Debug)]
pub enum IpcError {
    /// Creating, opening or mapping the shared memory or a doorbell failed.
    Io(std::io::Error),
    /// The address does not name a link made by this version of Suhina.
    BadLink(&'static str),
    /// Channel count or block size out of range.
    BadSize { channels: u32, max_frames: u32 },
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "shared memory: {e}"),
            Self::BadLink(why) => write!(f, "not a usable link: {why}"),
            Self::BadSize { channels, max_frames } => {
                write!(f, "{channels} channels of {max_frames} frames is not supported")
            }
        }
    }
}

impl std::error::Error for IpcError {}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// What happened to one block on the engine side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The host answered in time; the output holds its result.
    Done,
    /// The host did not answer before the deadline. The output is untouched.
    Missed,
    /// The host has not answered an earlier block yet, so this one was not sent. The output is
    /// untouched.
    Busy,
}

/// The engine's end of a link.
pub struct Link {
    region: Region,
    to_host: Doorbell,
    to_engine: Doorbell,
    /// The last block sent.
    sequence: u32,
    /// Samples of the block sent and not yet collected, if any.
    in_flight: Option<usize>,
}

impl Link {
    /// Creates a link for blocks of up to `max_frames` frames of `channels` interleaved samples.
    pub fn create(channels: u32, max_frames: u32) -> Result<Self, IpcError> {
        if !(1..=MAX_CHANNELS).contains(&channels) || !(1..=MAX_FRAMES).contains(&max_frames) {
            return Err(IpcError::BadSize { channels, max_frames });
        }
        let region = Region::create(channels, max_frames)?;
        let to_host = Doorbell::create(region.name(), "host")?;
        let to_engine = Doorbell::create(region.name(), "engine")?;
        Ok(Self { region, to_host, to_engine, sequence: 0, in_flight: None })
    }

    /// What the host process needs to open this link; pass it on its command line.
    pub fn address(&self) -> String {
        self.region.address().to_string()
    }

    /// The host has opened the link (it answered the load that named it), so the address is not
    /// needed again: where the memory is a named file (macOS), its name goes now and a crash
    /// leaves nothing behind. Elsewhere it has no name to remove. Not real-time safe (it may
    /// remove a file); call it where the link is made.
    pub fn host_opened(&mut self) {
        self.region.unlink();
    }

    pub fn channels(&self) -> u32 {
        self.region.channels()
    }

    pub fn max_frames(&self) -> u32 {
        self.region.max_frames()
    }

    /// Sends one block of interleaved `input` to the host and waits until `timeout` has passed
    /// for its `output`. The block is the shorter of the two, cut to whole frames and to the
    /// link's block size. Real-time safe: it does not allocate, and it returns within `timeout`.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], timeout: Duration) -> Outcome {
        self.process_with_events(input, &[], output, timeout)
    }

    /// [`Link::process`] with `events` at frames of the block (the first [`MAX_EVENTS`]; frames
    /// past the block's end count as its last frame).
    pub fn process_with_events(
        &mut self,
        input: &[f32],
        events: &[Event],
        output: &mut [f32],
        timeout: Duration,
    ) -> Outcome {
        let deadline = Instant::now() + timeout;
        if !self.send_with_events(input, events, output.len()) {
            return Outcome::Busy;
        }
        self.wait(output, deadline)
    }

    /// The first half of [`Link::process_with_events`]: sends the block (at most `room`
    /// samples of it, the output's size) and returns at once, so the blocks of several
    /// plug-ins can be out at the same time. `false` when the host has not answered an earlier
    /// block yet (the block is not sent). Real-time safe.
    pub fn send_with_events(&mut self, input: &[f32], events: &[Event], room: usize) -> bool {
        self.send_joint(input, events, &[], room)
    }

    /// [`Link::send_with_events`] for a block that the instances `followers` (numbers the
    /// host process gave them, the first [`MAX_FOLLOWERS`]) run after this link's own, one
    /// after another, each with the same `events`, before the host answers: the answer is the
    /// last one's output. Real-time safe.
    pub fn send_joint(&mut self, input: &[f32], events: &[Event], followers: &[u32], room: usize) -> bool {
        if !self.ready() {
            return false;
        }
        let channels = self.region.channels() as usize;
        let samples = input.len().min(room).min(self.region.samples());
        let samples = samples - samples % channels;
        self.region.write_input(input.get(..samples).unwrap_or_default());
        self.region.set_frames((samples / channels) as u32);
        self.region.write_events(events);
        self.region.write_followers(followers);
        self.sequence = self.sequence.wrapping_add(1);
        self.region.set_request(self.sequence);
        self.to_host.ring(self.region.request_word());
        self.in_flight = Some(samples);
        true
    }

    /// Whether a block can go now: the host has answered every block sent and none is out.
    /// Real-time safe.
    pub fn ready(&self) -> bool {
        self.region.response() == self.sequence && self.in_flight.is_none()
    }

    /// Waits until `deadline` for the host to answer the last block sent, one whose answer was
    /// given up on ([`Link::wait`] said `Missed`), so the next can go. `true` when the link is
    /// free; `false` at the deadline, or while a block is out (its [`Link::wait`] comes first).
    /// Real-time safe.
    pub fn wait_ready(&self, deadline: Instant) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        let sequence = self.sequence;
        self.region.response() == sequence
            || self.to_engine.wait(self.region.response_word(), |v| v == sequence, deadline)
    }

    /// Whether the block out has been answered, waiting for it at most until `until` (one
    /// already past only looks), without taking its result: [`Link::wait`] does that, at once
    /// after `true`. `true` too when no block is out, as there is nothing to wait for. Lets the
    /// engine look at the blocks of several links in turn and go on with whichever is answered
    /// first. Real-time safe.
    pub fn wait_answered(&self, until: Instant) -> bool {
        if self.in_flight.is_none() {
            return true;
        }
        let sequence = self.sequence;
        if self.region.response() == sequence {
            return true;
        }
        Instant::now() < until && self.to_engine.wait(self.region.response_word(), |v| v == sequence, until)
    }

    /// The second half: waits until `deadline` for the block sent with
    /// [`Link::send_with_events`] and reads its result into `output`. `Busy` when no block is
    /// out. Real-time safe.
    pub fn wait(&mut self, output: &mut [f32], deadline: Instant) -> Outcome {
        let Some(samples) = self.in_flight.take() else { return Outcome::Busy };
        let sequence = self.sequence;
        if self.to_engine.wait(self.region.response_word(), |v| v == sequence, deadline) {
            self.region.read_output(output.get_mut(..samples).unwrap_or_default());
            Outcome::Done
        } else {
            Outcome::Missed
        }
    }
}

/// The host's end of a link.
pub struct Served {
    region: Region,
    to_host: Doorbell,
    to_engine: Doorbell,
    /// The last block answered.
    seen: u32,
    /// The current block's events; room for [`MAX_EVENTS`], so serving does not allocate.
    events: Vec<Event>,
    /// The current block's followers; room for [`MAX_FOLLOWERS`].
    followers: Vec<u32>,
}

impl Served {
    /// Opens the link at `address` ([`Link::address`]).
    pub fn open(address: &str) -> Result<Self, IpcError> {
        let region = Region::open(address)?;
        let to_host = Doorbell::open(region.name(), "host")?;
        let to_engine = Doorbell::open(region.name(), "engine")?;
        let seen = region.response();
        Ok(Self {
            region,
            to_host,
            to_engine,
            seen,
            events: Vec::with_capacity(MAX_EVENTS),
            followers: Vec::with_capacity(MAX_FOLLOWERS),
        })
    }

    pub fn channels(&self) -> u32 {
        self.region.channels()
    }

    pub fn max_frames(&self) -> u32 {
        self.region.max_frames()
    }

    /// Waits until `timeout` has passed for a block, runs `process(input, output)` on it
    /// (interleaved, the same length, the output zeroed) and answers. Returns whether a block
    /// was served.
    pub fn serve(&mut self, timeout: Duration, process: impl FnOnce(&[f32], &mut [f32])) -> bool {
        self.serve_with_events(timeout, |input, _, output| process(input, output))
    }

    /// [`Served::serve`] that also hands over the block's events, in the order sent, each at a
    /// frame within the block.
    pub fn serve_with_events(&mut self, timeout: Duration, process: impl FnOnce(&[f32], &[Event], &mut [f32])) -> bool {
        self.serve_joint(timeout, |input, events, _, output| process(input, events, output))
    }

    /// [`Served::serve_with_events`] that also hands over the instances the engine asked to
    /// run the block after this link's own ([`Link::send_joint`]), in order; empty for most
    /// blocks. `process` runs them all and leaves the last one's result in the output.
    pub fn serve_joint(
        &mut self,
        timeout: Duration,
        process: impl FnOnce(&[f32], &[Event], &[u32], &mut [f32]),
    ) -> bool {
        let deadline = Instant::now() + timeout;
        let seen = self.seen;
        if !self.to_host.wait(self.region.request_word(), |v| v != seen, deadline) {
            return false;
        }
        let request = self.region.request();
        let frames = self.region.frames();
        let samples = frames as usize * self.region.channels() as usize;
        let mut events = std::mem::take(&mut self.events);
        self.region.read_events(&mut events);
        let last = frames.saturating_sub(1);
        events.iter_mut().for_each(|e| e.frame = e.frame.min(last));
        let mut followers = std::mem::take(&mut self.followers);
        self.region.read_followers(&mut followers);
        self.region.with_block(samples, |input, output| process(input, &events, &followers, output));
        self.events = events;
        self.followers = followers;
        self.seen = request;
        self.region.set_response(request);
        self.to_engine.ring(self.region.response_word());
        true
    }
}

#[cfg(test)]
mod tests;
