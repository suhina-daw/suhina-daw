// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shared-memory region: a header of atomics, then the input and the output buffer, then
//! the block's events, then the instances that run the block after this link's own (its
//! followers).
//!
//! The region is a file mapped by both processes (in `/dev/shm` on Linux, so it stays in
//! memory; in the temporary directory elsewhere). The engine creates it and removes it when the
//! link is dropped.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use memmap2::MmapMut;

use crate::events::{EVENT_BYTES, Event, MAX_EVENTS};
use crate::{IpcError, MAX_CHANNELS, MAX_FOLLOWERS, MAX_FRAMES};

const MAGIC: u32 = 0x4C57_4950;
const VERSION: u32 = 3;
/// The header's size; the buffers start after it.
const HEADER_BYTES: usize = 256;
// Byte offsets of the header fields. The words the two sides write sit on cache lines of their
// own.
const AT_MAGIC: usize = 0;
const AT_VERSION: usize = 4;
const AT_CHANNELS: usize = 8;
const AT_MAX_FRAMES: usize = 12;
const AT_REQUEST: usize = 64;
const AT_RESPONSE: usize = 128;
const AT_FRAMES: usize = 192;
const AT_EVENTS: usize = 196;
/// Bytes of the events area.
const EVENTS_BYTES: usize = MAX_EVENTS * EVENT_BYTES;
/// Bytes of the followers area: their count, then each one's instance number.
const FOLLOWERS_BYTES: usize = 4 * (1 + MAX_FOLLOWERS);

/// Bytes a region for `channels` × `max_frames` needs: the header, two buffers, the events and
/// the followers.
fn size(channels: u32, max_frames: u32) -> Option<usize> {
    let samples = (channels as usize).checked_mul(max_frames as usize)?;
    samples
        .checked_mul(2 * size_of::<f32>())?
        .checked_add(HEADER_BYTES)?
        .checked_add(EVENTS_BYTES)?
        .checked_add(FOLLOWERS_BYTES)
}

/// Where new regions go.
pub(crate) fn directory() -> PathBuf {
    let shm = Path::new("/dev/shm");
    if cfg!(target_os = "linux") && shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() }
}

pub(crate) struct Region {
    map: MmapMut,
    path: PathBuf,
    name: String,
    channels: u32,
    max_frames: u32,
    /// Whether this side created the region and removes it.
    owner: bool,
}

impl Region {
    pub(crate) fn create(channels: u32, max_frames: u32) -> Result<Self, IpcError> {
        let len = size(channels, max_frames).ok_or(IpcError::BadSize { channels, max_frames })?;
        let name = unique_name();
        let path = directory().join(&name);
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
        file.set_len(len as u64)?;
        let mut region = Self { map: map(&file)?, path, name, channels, max_frames, owner: true };
        region.map.fill(0);
        region.word(AT_MAGIC).store(MAGIC, Ordering::Relaxed);
        region.word(AT_VERSION).store(VERSION, Ordering::Relaxed);
        region.word(AT_CHANNELS).store(channels, Ordering::Relaxed);
        region.word(AT_MAX_FRAMES).store(max_frames, Ordering::Release);
        Ok(region)
    }

    pub(crate) fn open(path: &Path) -> Result<Self, IpcError> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| n.starts_with("suhina-ipc-"))
            .ok_or(IpcError::BadLink("not a Suhina link name"))?
            .to_string();
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        if (file.metadata()?.len() as usize) < HEADER_BYTES {
            return Err(IpcError::BadLink("too short"));
        }
        let map = map(&file)?;
        let mut region = Self { map, path: path.to_path_buf(), name, channels: 0, max_frames: 0, owner: false };
        if region.word(AT_MAGIC).load(Ordering::Acquire) != MAGIC {
            return Err(IpcError::BadLink("wrong magic number"));
        }
        if region.word(AT_VERSION).load(Ordering::Acquire) != VERSION {
            return Err(IpcError::BadLink("made by another version"));
        }
        let channels = region.word(AT_CHANNELS).load(Ordering::Acquire);
        let max_frames = region.word(AT_MAX_FRAMES).load(Ordering::Acquire);
        if !(1..=MAX_CHANNELS).contains(&channels) || !(1..=MAX_FRAMES).contains(&max_frames) {
            return Err(IpcError::BadSize { channels, max_frames });
        }
        if size(channels, max_frames).is_none_or(|needed| needed > region.map.len()) {
            return Err(IpcError::BadLink("shorter than its sizes say"));
        }
        region.channels = channels;
        region.max_frames = max_frames;
        Ok(region)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn channels(&self) -> u32 {
        self.channels
    }

    pub(crate) fn max_frames(&self) -> u32 {
        self.max_frames
    }

    /// Samples in each buffer.
    pub(crate) fn samples(&self) -> usize {
        self.channels as usize * self.max_frames as usize
    }

    /// The atomic word at byte `offset` of the header.
    fn word(&self, offset: usize) -> &AtomicU32 {
        let ptr = self.map.as_ptr().wrapping_add(offset).cast::<AtomicU32>();
        // SAFETY: `offset` is one of the header constants, inside the mapping (which is at
        // least `HEADER_BYTES` long, checked on create and open) and 4-byte aligned (the
        // mapping is page-aligned). The other process only ever touches these bytes through
        // atomics too, and `AtomicU32` allows shared mutation through `&`.
        unsafe { &*ptr }
    }

    pub(crate) fn request_word(&self) -> &AtomicU32 {
        self.word(AT_REQUEST)
    }

    pub(crate) fn response_word(&self) -> &AtomicU32 {
        self.word(AT_RESPONSE)
    }

    pub(crate) fn request(&self) -> u32 {
        self.request_word().load(Ordering::Acquire)
    }

    pub(crate) fn response(&self) -> u32 {
        self.response_word().load(Ordering::Acquire)
    }

    pub(crate) fn set_request(&self, value: u32) {
        self.request_word().store(value, Ordering::Release);
    }

    pub(crate) fn set_response(&self, value: u32) {
        self.response_word().store(value, Ordering::Release);
    }

    /// Frames in the current block; at most `max_frames`, whatever the other side wrote.
    pub(crate) fn frames(&self) -> u32 {
        self.word(AT_FRAMES).load(Ordering::Acquire).min(self.max_frames)
    }

    pub(crate) fn set_frames(&self, frames: u32) {
        self.word(AT_FRAMES).store(frames, Ordering::Release);
    }

    /// Start of buffer `index` (0 input, 1 output).
    fn buffer(&mut self, index: usize) -> *mut f32 {
        let offset = HEADER_BYTES + index * self.samples() * size_of::<f32>();
        self.map.as_mut_ptr().wrapping_add(offset).cast::<f32>()
    }

    /// Copies `samples` (at most `samples()` long) into the input buffer.
    pub(crate) fn write_input(&mut self, samples: &[f32]) {
        let len = samples.len().min(self.samples());
        let to = self.buffer(0);
        // SAFETY: the input buffer has room for `samples()` values, `len` is at most that, the
        // source is a separate Rust slice so they do not overlap, and the buffer is 4-byte
        // aligned (after a 256-byte header in a page-aligned mapping). The protocol gives the
        // engine the buffer alone: the host does not read it until the request is published,
        // which happens after this copy.
        unsafe { std::ptr::copy_nonoverlapping(samples.as_ptr(), to, len) };
    }

    /// Copies the output buffer's first `out.len()` values (at most `samples()`) into `out`.
    pub(crate) fn read_output(&mut self, out: &mut [f32]) {
        let len = out.len().min(self.samples());
        let from = self.buffer(1);
        // SAFETY: as in `write_input`, the other way: the output buffer holds `samples()`
        // values, `len` is at most that, and the host has finished with it (its response was
        // read with Acquire ordering before this copy).
        unsafe { std::ptr::copy_nonoverlapping(from, out.as_mut_ptr(), len) };
    }

    /// Start of the events area, after both buffers.
    fn events_area(&mut self) -> *mut u8 {
        let offset = HEADER_BYTES + 2 * self.samples() * size_of::<f32>();
        self.map.as_mut_ptr().wrapping_add(offset)
    }

    /// Stores the block's `events` (the first [`MAX_EVENTS`]) and their count.
    pub(crate) fn write_events(&mut self, events: &[Event]) {
        let count = events.len().min(MAX_EVENTS);
        let area = self.events_area();
        for (index, event) in events.iter().take(count).enumerate() {
            let bytes = event.encode();
            let to = area.wrapping_add(index * EVENT_BYTES);
            // SAFETY: the events area holds `MAX_EVENTS` records of `EVENT_BYTES` (the region's
            // size includes it, checked on create and open) and `index` is below that; the
            // source is a local array. As with the input buffer, the host does not read the
            // area until the request is published, after this copy.
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), to, EVENT_BYTES) };
        }
        self.word(AT_EVENTS).store(count as u32, Ordering::Release);
    }

    /// Reads the block's events into `out` (emptied first; it should have room for
    /// [`MAX_EVENTS`], so this does not allocate). Records of an unknown kind are skipped.
    pub(crate) fn read_events(&mut self, out: &mut Vec<Event>) {
        out.clear();
        let count = (self.word(AT_EVENTS).load(Ordering::Acquire) as usize).min(MAX_EVENTS);
        let area = self.events_area();
        for index in 0..count {
            let mut bytes = [0u8; EVENT_BYTES];
            let from = area.wrapping_add(index * EVENT_BYTES);
            // SAFETY: as in `write_events`, the other way: the record is inside the area, and
            // the engine wrote it before publishing the request this block answers.
            unsafe { std::ptr::copy_nonoverlapping(from, bytes.as_mut_ptr(), EVENT_BYTES) };
            if let Some(event) = Event::decode(&bytes) {
                out.push(event);
            }
        }
    }

    /// Start of the followers area, after the events.
    fn followers_area(&mut self) -> *mut u8 {
        let offset = HEADER_BYTES + 2 * self.samples() * size_of::<f32>() + EVENTS_BYTES;
        self.map.as_mut_ptr().wrapping_add(offset)
    }

    /// Stores the instances that run the block after this link's own (the first
    /// [`MAX_FOLLOWERS`]), in order, and their count.
    pub(crate) fn write_followers(&mut self, followers: &[u32]) {
        let count = followers.len().min(MAX_FOLLOWERS);
        let area = self.followers_area();
        for (index, follower) in followers.iter().take(count).enumerate() {
            let bytes = follower.to_le_bytes();
            let to = area.wrapping_add(4 * (1 + index));
            // SAFETY: the followers area holds a count and `MAX_FOLLOWERS` numbers of 4 bytes
            // (the region's size includes it, checked on create and open) and `index` is below
            // that; the source is a local array. The host does not read the area until the
            // request is published, after this copy.
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), to, 4) };
        }
        let bytes = (count as u32).to_le_bytes();
        // SAFETY: as above, the count's 4 bytes at the start of the area.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), area, 4) };
    }

    /// Reads the block's followers into `out` (emptied first; it should have room for
    /// [`MAX_FOLLOWERS`], so this does not allocate).
    pub(crate) fn read_followers(&mut self, out: &mut Vec<u32>) {
        out.clear();
        let area = self.followers_area();
        let mut bytes = [0u8; 4];
        // SAFETY: as in `write_followers`, the other way: the count is inside the area, and
        // the engine wrote it before publishing the request this block answers.
        unsafe { std::ptr::copy_nonoverlapping(area, bytes.as_mut_ptr(), 4) };
        let count = (u32::from_le_bytes(bytes) as usize).min(MAX_FOLLOWERS);
        for index in 0..count {
            let from = area.wrapping_add(4 * (1 + index));
            // SAFETY: as above; `index` is below `MAX_FOLLOWERS`.
            unsafe { std::ptr::copy_nonoverlapping(from, bytes.as_mut_ptr(), 4) };
            out.push(u32::from_le_bytes(bytes));
        }
    }

    /// Runs `process` on the first `samples` values of both buffers, the output zeroed first.
    pub(crate) fn with_block(&mut self, samples: usize, process: impl FnOnce(&[f32], &mut [f32])) {
        let len = samples.min(self.samples());
        let (input, output) = (self.buffer(0), self.buffer(1));
        // SAFETY: both buffers hold `samples()` values and `len` is at most that; they do not
        // overlap (the output starts where the input ends); they are 4-byte aligned. Every bit
        // pattern is a valid f32. The protocol gives the host both buffers alone between the
        // engine's request and the host's response, and the slices do not outlive this call.
        let input = unsafe { std::slice::from_raw_parts(input, len) };
        // SAFETY: as above; the output slice is the only reference to the output buffer.
        let output = unsafe { std::slice::from_raw_parts_mut(output, len) };
        output.fill(0.0);
        process(input, output);
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        if self.owner {
            // A host that still has it mapped keeps its view; on Windows the file stays until it
            // closes. Nothing to do about a failure here.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn map(file: &File) -> Result<MmapMut, IpcError> {
    // SAFETY: mapping a file is unsafe because another process may change it underneath. That
    // is the point here: the only other writer is the peer of this link, and every access
    // goes through atomics or through the buffers the protocol hands to one side at a time.
    Ok(unsafe { MmapMut::map_mut(file) }?)
}

/// A file name no other link uses: the process id, a counter and the time.
fn unique_name() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    format!("suhina-ipc-{}-{count}-{nanos}", std::process::id())
}
