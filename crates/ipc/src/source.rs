// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio source in shared memory, for an ARA plug-in to read (ADR 0033 decision 4): the
//! engine side writes a file's decoded samples once, the host process maps them read only and
//! serves the plug-in's reads from them, with no round trip and no file I/O.
//!
//! Layout: a 64-byte header (magic, version, channel count, frame count, sample rate), then
//! each channel's samples in turn as 32-bit floats (planar, as ARA reads them).
//!
//! The memory is freed by the operating system when both sides have let go of it
//! ([`crate::shm`]). The host may open a source again while the engine side holds it (a
//! restarted host does), so on macOS its file keeps its name until then.

use crate::shm::{Access, Shared};
use crate::{IpcError, MAX_CHANNELS};

/// What the shared memory of a source is called ([`Shared`]).
const KIND: &str = "ara";

const MAGIC: u32 = 0x5341_5241; // "ARAS"
const VERSION: u32 = 1;
const HEADER_BYTES: usize = 64;
/// Longest source, in frames: about six hours at 48 kHz.
pub const MAX_SOURCE_FRAMES: u64 = 1 << 30;

fn size(channels: u32, frames: u64) -> Option<usize> {
    let samples = u64::from(channels).checked_mul(frames)?;
    usize::try_from(samples.checked_mul(4)?).ok()?.checked_add(HEADER_BYTES)
}

/// One audio source's samples in shared memory.
pub struct AudioSource {
    map: Shared,
    channels: u32,
    frames: u64,
    sample_rate: f64,
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
        let shared = Shared::create(KIND, len)?;
        // SAFETY: memory this process just made, at least `len` bytes long; nobody else maps it
        // until its address is handed on, after it is written, and this slice ends before that.
        let map = unsafe { std::slice::from_raw_parts_mut(shared.as_ptr(), len) };
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
        Ok(Self { map: shared, channels, frames, sample_rate })
    }

    /// Opens a region [`AudioSource::from_interleaved`] made, read only. Everything in its
    /// header is checked against the file's length.
    pub fn open(address: &str) -> Result<Self, IpcError> {
        let shared = Shared::open(KIND, address, Access::Read)?;
        let map = bytes(&shared);
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
        Ok(Self { map: shared, channels, frames, sample_rate })
    }

    /// What the host process needs to open it.
    pub fn address(&self) -> String {
        self.map.address().to_string()
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
        let bytes = bytes(&self.map);
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

/// The source's bytes, header and samples.
fn bytes(map: &Shared) -> &[u8] {
    // SAFETY: the mapping is `len()` bytes long and lives as long as `map`. The engine side wrote
    // it before handing on its address and does not write it again, so nothing changes it while
    // it is read; the host side mapped it read only.
    unsafe { std::slice::from_raw_parts(map.as_ptr().cast_const(), map.len()) }
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
        // It opens again while the engine side holds it, as for a restarted host.
        let again = AudioSource::open(&source.address()).unwrap();
        assert_eq!(again.frames(), 3);
        // The memory goes with the side that made it. (On Linux the address is a descriptor
        // number, which another test may take next, so it is not tried again there.)
        let address = source.address();
        drop((opened, again, source));
        #[cfg(not(target_os = "linux"))]
        assert!(AudioSource::open(&address).is_err());
        let _ = address;
    }

    #[test]
    fn bad_regions_are_refused() {
        assert!(AudioSource::from_interleaved(&[0.0; 4], 0, 48_000.0).is_err());
        assert!(AudioSource::from_interleaved(&[0.0; 4], 2, f64::NAN).is_err());
        // Names and paths that are not a source's.
        let own = std::process::id();
        for address in [
            "/tmp/not-a-source".to_string(),
            format!("/proc/{own}/fd/0"),
            format!("/proc/{own}/fd/x"),
            "Local\\suhina-ara-1-2-3".to_string(),
            String::new(),
        ] {
            assert!(AudioSource::open(&address).is_err(), "{address}");
        }
        let source = AudioSource::from_interleaved(&[0.5; 8], 2, 44_100.0).unwrap();
        let mut bytes = bytes(&source.map)[..HEADER_BYTES + 32].to_vec();
        // A link's region is not a source, whatever it holds.
        let region = crate::shm::with_bytes("ipc", &bytes);
        assert!(AudioSource::open(region.address()).is_err());
        // A header that claims more frames than the memory holds.
        bytes[16..24].copy_from_slice(&1_000_000_000u64.to_le_bytes());
        let long = crate::shm::with_bytes(KIND, &bytes);
        assert!(AudioSource::open(long.address()).is_err());
        bytes[0] = 0;
        let wrong = crate::shm::with_bytes(KIND, &bytes);
        assert!(AudioSource::open(wrong.address()).is_err());
    }
}
