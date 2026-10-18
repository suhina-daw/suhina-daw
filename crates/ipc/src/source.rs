// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio source in shared memory, for an ARA plug-in to read (ADR 0028 decision 4): the
//! engine side writes a file's decoded samples once, the host process maps them read only and
//! serves the plug-in's reads from them, with no round trip and no file I/O.
//!
//! Layout: a 64-byte header (magic, version, channel count, frame count, sample rate), then
//! each channel's samples in turn as 32-bit floats (planar, as ARA reads them).

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use memmap2::{Mmap, MmapMut};

use crate::{IpcError, MAX_CHANNELS};

const MAGIC: u32 = 0x5341_5241; // "ARAS"
const VERSION: u32 = 1;
const HEADER_BYTES: usize = 64;
/// Longest source, in frames: about six hours at 48 kHz.
pub const MAX_SOURCE_FRAMES: u64 = 1 << 30;

fn size(channels: u32, frames: u64) -> Option<usize> {
    let samples = u64::from(channels).checked_mul(frames)?;
    usize::try_from(samples.checked_mul(4)?).ok()?.checked_add(HEADER_BYTES)
}

enum Map {
    Write(MmapMut),
    Read(Mmap),
}

impl Map {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Write(map) => map,
            Self::Read(map) => map,
        }
    }
}

/// One audio source's samples in shared memory.
pub struct AudioSource {
    map: Map,
    path: PathBuf,
    channels: u32,
    frames: u64,
    sample_rate: f64,
    /// Whether this side made the file and removes it.
    owner: bool,
}

impl AudioSource {
    /// Puts `samples` (interleaved, `channels` per frame, at `sample_rate`) into a new region.
    pub fn from_interleaved(samples: &[f32], channels: u32, sample_rate: f64) -> Result<Self, IpcError> {
        if !(1..=MAX_CHANNELS).contains(&channels) || !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(IpcError::BadLink("not a usable audio source"));
        }
        let frames = (samples.len() / channels as usize) as u64;
        if frames > MAX_SOURCE_FRAMES {
            return Err(IpcError::BadLink("audio source too long"));
        }
        let len = size(channels, frames).ok_or(IpcError::BadLink("audio source too long"))?;
        let path = crate::region::directory().join(unique_name());
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
        file.set_len(len as u64)?;
        // SAFETY: a file this process just made; nobody else maps it until its address is
        // handed on, after it is written.
        let mut map = unsafe { MmapMut::map_mut(&file) }?;
        map[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        map[4..8].copy_from_slice(&VERSION.to_le_bytes());
        map[8..12].copy_from_slice(&channels.to_le_bytes());
        map[16..24].copy_from_slice(&frames.to_le_bytes());
        map[24..32].copy_from_slice(&sample_rate.to_le_bytes());
        let body = &mut map[HEADER_BYTES..];
        let channels_n = channels as usize;
        for (frame, values) in samples.chunks_exact(channels_n).enumerate() {
            for (channel, value) in values.iter().enumerate() {
                let at = (channel * frames as usize + frame) * 4;
                if let Some(slot) = body.get_mut(at..at + 4) {
                    slot.copy_from_slice(&value.to_le_bytes());
                }
            }
        }
        map.flush()?;
        Ok(Self { map: Map::Write(map), path, channels, frames, sample_rate, owner: true })
    }

    /// Opens a region [`AudioSource::from_interleaved`] made, read only. Everything in its
    /// header is checked against the file's length.
    pub fn open(address: &str) -> Result<Self, IpcError> {
        let path = Path::new(address);
        let named = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("suhina-ara-"));
        if !named {
            return Err(IpcError::BadLink("not a Suhina audio source"));
        }
        let file = File::open(path)?;
        // SAFETY: the engine side wrote the file before handing on its address and does not
        // write it again; this side only reads it.
        let map = unsafe { Mmap::map(&file) }?;
        let word = |at: usize| map.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
        let long = |at: usize| map.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
        if word(0) != Some(MAGIC) {
            return Err(IpcError::BadLink("wrong magic number"));
        }
        if word(4) != Some(VERSION) {
            return Err(IpcError::BadLink("made by another version"));
        }
        let channels = word(8).unwrap_or(0);
        let frames = long(16).unwrap_or(u64::MAX);
        let sample_rate = f64::from_bits(long(24).unwrap_or(0));
        let fits = size(channels, frames).is_some_and(|needed| needed <= map.len());
        if !(1..=MAX_CHANNELS).contains(&channels) || frames > MAX_SOURCE_FRAMES || !fits {
            return Err(IpcError::BadLink("audio source sizes do not fit"));
        }
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(IpcError::BadLink("audio source has no sample rate"));
        }
        Ok(Self { map: Map::Read(map), path: path.to_path_buf(), channels, frames, sample_rate, owner: false })
    }

    /// What the host process needs to open it.
    pub fn address(&self) -> String {
        self.path.display().to_string()
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Fills `out` with `channel`'s samples from frame `position` on; frames outside the source
    /// (before 0, after its end, or a channel it does not have) are silence.
    pub fn read(&self, channel: u32, position: i64, out: &mut [f32]) {
        out.fill(0.0);
        if channel >= self.channels {
            return;
        }
        let bytes = self.map.bytes();
        let base = HEADER_BYTES + channel as usize * self.frames as usize * 4;
        for (index, value) in out.iter_mut().enumerate() {
            let Some(frame) = position.checked_add(index as i64) else { break };
            if frame < 0 {
                continue;
            }
            if frame as u64 >= self.frames {
                break;
            }
            let at = base + frame as usize * 4;
            if let Some(b) = bytes.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()) {
                *value = f32::from_le_bytes(b);
            }
        }
    }
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        if self.owner {
            // A host that still has it mapped keeps its view; on Windows the file stays until it
            // lets go of it.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn unique_name() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    format!("suhina-ara-{}-{count}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_reads_back_planar_with_silence_outside() {
        // Three stereo frames: left 1, 2, 3 and right -1, -2, -3.
        let source = AudioSource::from_interleaved(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0], 2, 48_000.0).unwrap();
        let opened = AudioSource::open(&source.address()).unwrap();
        assert_eq!((opened.channels(), opened.frames(), opened.sample_rate()), (2, 3, 48_000.0));
        let mut out = [9.0; 5];
        opened.read(0, -1, &mut out);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 0.0]);
        opened.read(1, 1, &mut out);
        assert_eq!(out, [-2.0, -3.0, 0.0, 0.0, 0.0]);
        opened.read(2, 0, &mut out);
        assert_eq!(out, [0.0; 5]);
        opened.read(0, i64::MAX, &mut out);
        assert_eq!(out, [0.0; 5]);
        // The file goes with the side that made it.
        let address = source.address();
        drop(opened);
        drop(source);
        assert!(!Path::new(&address).exists());
    }

    #[test]
    fn bad_regions_are_refused() {
        assert!(AudioSource::from_interleaved(&[0.0; 4], 0, 48_000.0).is_err());
        assert!(AudioSource::from_interleaved(&[0.0; 4], 2, f64::NAN).is_err());
        assert!(AudioSource::open("/tmp/not-a-source").is_err());
        let source = AudioSource::from_interleaved(&[0.5; 8], 2, 44_100.0).unwrap();
        // A header that claims more frames than the file holds.
        let path = source.address();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[16..24].copy_from_slice(&1_000_000u64.to_le_bytes());
        let copy = crate::region::directory().join(unique_name());
        std::fs::write(&copy, &bytes).unwrap();
        assert!(AudioSource::open(&copy.display().to_string()).is_err());
        bytes[0] = 0;
        std::fs::write(&copy, &bytes).unwrap();
        assert!(AudioSource::open(&copy.display().to_string()).is_err());
        let _ = std::fs::remove_file(copy);
    }
}
