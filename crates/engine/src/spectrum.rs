// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The device-spectrum rings the EQ panel's analyser reads (docs/spec/eq.md › The panel › The
//! analyser), carried by the meter monitor ([`crate::meters::MeterMonitor`]). The audio
//! thread's chain pass mixes a watched device's output block to mono and appends it to a ring
//! of atomics; the UI reads the ring and does the window, the transform and the bands itself,
//! so the audio thread's whole cost is the mixdown and the stores, and only while a panel
//! watches. Nothing here allocates, locks or waits.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use suhina_model::TrackId;

/// How many devices can be watched at once: a panel claims the row `device index mod ROWS`.
pub const ROWS: usize = 16;
/// The ring's length in frames, mono: about 43 ms at 48 kHz.
pub const RING: usize = 2048;
/// The track id of a watch row nobody claims.
const NO_WATCH: u32 = u32::MAX;

/// Which device of which track a row watches, as atomics the UI writes and the audio thread
/// reads.
#[derive(Debug)]
struct Watch {
    track: AtomicU32,
    device: AtomicU32,
}

/// One row's ring: the mono samples of the watched device's output, as `f32` bits, and the
/// count of frames ever appended (the write cursor). The audio thread writes; the UI reads.
#[derive(Debug)]
struct Ring {
    written: AtomicUsize,
    samples: Box<[AtomicU32]>,
}

/// The watched devices' output rings. Allocated once with the meter monitor, off the audio
/// thread; after that every access is an atomic load or store.
#[derive(Debug)]
pub struct Spectrum {
    watches: Box<[Watch]>,
    rings: Box<[Ring]>,
    /// The rate the rings are filled at, as the engine's renderer was built with; 0 before it
    /// is set.
    rate: AtomicU32,
}

impl Spectrum {
    pub(crate) fn new() -> Self {
        let watch = || Watch { track: AtomicU32::new(NO_WATCH), device: AtomicU32::new(0) };
        let ring = || Ring {
            written: AtomicUsize::new(0),
            samples: std::iter::repeat_with(|| AtomicU32::new(0.0f32.to_bits())).take(RING).collect(),
        };
        Self {
            watches: (0..ROWS).map(|_| watch()).collect(),
            rings: (0..ROWS).map(|_| ring()).collect(),
            rate: AtomicU32::new(0),
        }
    }

    /// The engine's renderer calls this once, at its construction, so the UI can place the
    /// bands on the frequency axis.
    pub(crate) fn set_rate(&self, sample_rate: u32) {
        self.rate.store(sample_rate, Ordering::Relaxed);
    }

    /// The rate the rings are filled at, or the engine's default before it is set.
    pub fn sample_rate(&self) -> f64 {
        match self.rate.load(Ordering::Relaxed) {
            0 => 48_000.0,
            rate => f64::from(rate),
        }
    }

    /// The UI claims `row` for the device at `device` of `track` while its panel is drawn;
    /// claiming it again, or another panel claiming the same row, simply overwrites.
    pub fn watch(&self, row: usize, track: TrackId, device: usize) {
        let Ok(device) = u32::try_from(device) else { return };
        if let Some(watch) = self.watches.get(row % ROWS) {
            watch.track.store(track.0, Ordering::Relaxed);
            watch.device.store(device, Ordering::Relaxed);
        }
    }

    /// The device index each row watches of `track`'s chain: the audio thread's snapshot for
    /// one block. A row that watches another track reads as `None`.
    pub(crate) fn watched(&self, track: TrackId) -> [Option<usize>; ROWS] {
        let mut rows = [None; ROWS];
        for (index, watch) in self.watches.iter().enumerate() {
            if watch.track.load(Ordering::Relaxed) == track.0 {
                rows[index] = usize::try_from(watch.device.load(Ordering::Relaxed)).ok();
            }
        }
        rows
    }

    /// The audio thread: `block` (interleaved stereo), the watched device's output this
    /// block, mixes to mono and appends to `row`'s ring. Never allocates.
    pub(crate) fn append(&self, row: usize, block: &[f32], frames: usize) {
        let Some(ring) = self.rings.get(row % ROWS) else { return };
        let cursor = ring.written.load(Ordering::Relaxed);
        for (index, frame) in block.as_chunks::<2>().0.iter().take(frames).enumerate() {
            let left = frame.first().copied().unwrap_or(0.0);
            let right = frame.get(1).copied().unwrap_or(0.0);
            let mono = (left + right) * 0.5;
            let mono = if mono.is_finite() { mono } else { 0.0 };
            if let Some(cell) = ring.samples.get((cursor + index) % RING) {
                cell.store(mono.to_bits(), Ordering::Relaxed);
            }
        }
        ring.written.store(cursor + frames, Ordering::Release);
    }

    /// The row that watches `(track, device)`, if any.
    fn row_of(&self, track: TrackId, device: u32) -> Option<usize> {
        self.watches.iter().position(|watch| {
            watch.track.load(Ordering::Relaxed) == track.0 && watch.device.load(Ordering::Relaxed) == device
        })
    }

    /// How many frames the ring of the device a row watches has ever been appended: a count
    /// that moves only while the engine renders the device, so a panel drawing its spectrum
    /// asks for the next frame while it moves and holds its last line when it stops. `None`
    /// while nothing watches `(track, device)` (docs/spec/eq.md › The panel › The analyser).
    pub fn appended(&self, track: TrackId, device: usize) -> Option<usize> {
        let device = u32::try_from(device).ok()?;
        Some(self.rings.get(self.row_of(track, device)?)?.written.load(Ordering::Acquire))
    }

    /// The UI reads a watched device's ring: the last `RING` mono samples, oldest first, and
    /// how many of them are real (the rest are silence, before the ring first filled), or
    /// `None` while nothing watches `(track, device)` or nothing was appended yet.
    pub fn samples(&self, track: TrackId, device: usize) -> Option<([f32; RING], usize)> {
        let device = u32::try_from(device).ok()?;
        let ring = self.rings.get(self.row_of(track, device)?)?;
        let written = ring.written.load(Ordering::Acquire).min(RING);
        if written == 0 {
            return None;
        }
        let oldest = ring.written.load(Ordering::Acquire) - written;
        let mut out = [0.0; RING];
        for (index, cell) in out.iter_mut().enumerate().skip(RING - written) {
            let at = (oldest + index - (RING - written)) % RING;
            *cell = ring.samples.get(at).map(|c| f32::from_bits(c.load(Ordering::Relaxed))).unwrap_or(0.0);
        }
        Some((out, written))
    }

    /// Drops every watch, as a graph swap does: the rows start from nobody watching.
    pub(crate) fn clear(&self) {
        for watch in self.watches.iter() {
            watch.track.store(NO_WATCH, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_scopes_the_ring_to_its_track_and_device() {
        let spectrum = Spectrum::new();
        assert!(spectrum.samples(TrackId(7), 2).is_none(), "nothing watched yet");
        spectrum.watch(0, TrackId(7), 2);
        assert_eq!(spectrum.watched(TrackId(7)), {
            let mut r = [None; ROWS];
            r[0] = Some(2);
            r
        });
        assert_eq!(spectrum.watched(TrackId(8)), [None; ROWS]);
        assert!(spectrum.samples(TrackId(8), 2).is_none(), "another track reads nothing");
        assert!(spectrum.samples(TrackId(7), 3).is_none(), "another device reads nothing");
        // A second claim of the row takes it over.
        spectrum.watch(0, TrackId(9), 1);
        assert!(spectrum.samples(TrackId(7), 2).is_none(), "the first claim is gone");
        spectrum.watch(3, TrackId(7), 2);
        spectrum.append(3, &[0.5, 0.0, 0.5, 0.0], 2);
        let (samples, valid) = spectrum.samples(TrackId(7), 2).unwrap();
        assert_eq!(valid, 2, "one block's frames are there");
        assert_eq!(samples[RING - 1], 0.25, "the last frame's mono mix, half of left plus right");
        assert_eq!(samples[0], 0.0, "the ring's front is silence until it first fills");
    }

    #[test]
    fn the_ring_holds_its_last_ring_frames_across_a_wrap() {
        let spectrum = Spectrum::new();
        spectrum.watch(1, TrackId(7), 0);
        // Three appends of the whole ring: the last append's samples are the newest, and the
        // block's frames alternate the round's level with silence.
        for round in 0..3 {
            let block: Vec<f32> = (0..RING * 2).map(|i| if i / 2 % 2 == 0 { round as f32 } else { 0.0 }).collect();
            spectrum.append(1, &block, RING);
        }
        let (samples, valid) = spectrum.samples(TrackId(7), 0).unwrap();
        assert_eq!(valid, RING);
        assert_eq!(samples[0], 2.0, "the oldest valid frame is the last append's first, the ring wrapped");
        assert_eq!(samples[RING - 2], 2.0);
        assert_eq!(samples[RING - 1], 0.0, "the block's last frame is a silence frame");
        // The reading is scoped to the watch: the row's other devices read nothing.
        assert!(spectrum.samples(TrackId(7), 1).is_none());
    }

    #[test]
    fn the_count_of_appended_frames_moves_with_every_append() {
        let spectrum = Spectrum::new();
        assert_eq!(spectrum.appended(TrackId(7), 2), None, "nothing watched yet");
        spectrum.watch(3, TrackId(7), 2);
        assert_eq!(spectrum.appended(TrackId(7), 2), Some(0), "watched, nothing appended yet");
        spectrum.append(3, &[0.5, 0.5, 0.25, 0.25], 2);
        assert_eq!(spectrum.appended(TrackId(7), 2), Some(2), "one block's frames are counted");
        spectrum.append(3, &[0.5, 0.5, 0.25, 0.25], 2);
        assert_eq!(spectrum.appended(TrackId(7), 2), Some(4), "the count moves with every append");
        assert_eq!(spectrum.appended(TrackId(8), 2), None, "another track reads nothing");
    }

    #[test]
    fn clearing_drops_the_watches_but_keeps_nothing_claimed() {
        let spectrum = Spectrum::new();
        spectrum.watch(2, TrackId(7), 5);
        spectrum.clear();
        assert_eq!(spectrum.watched(TrackId(7)), [None; ROWS]);
    }
}
