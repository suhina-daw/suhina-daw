// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the host can run. Until plug-in formats land, only test processors, named `test:…`:
//!
//! - `test:gain`: multiplies by a gain (0.5 at first); its state is the gain as 4 bytes,
//!   little-endian `f32`.
//! - `test:crash-after:<n>`: passes audio through, then exits the process in the middle of
//!   block `n` (counting from 0) without answering.
//! - `test:hang-after:<n>`: passes audio through, then stops answering in block `n`.
//! - `test:slow-load:<ms>`: `test:gain` that takes `ms` milliseconds (at most 60 s) to load, like
//!   a large plug-in.
//! - `test:slow-start:<n>`: `test:gain` whose first `n` blocks take 30 ms each, like a plug-in
//!   that has just started (or been restarted) and is still settling.
//! - `test:delay-after:<n>:<frames>`: passes audio through with no latency, then from block `n`
//!   (counting from 0) on delays it by `frames` (at most [`MAX_DELAY`]) and reports that as its
//!   latency, like a plug-in whose lookahead changes while it runs. Stereo, as the engine's
//!   links are.
//! - `test:busy:<µs>`: halves the sound, keeping its thread busy for `µs` microseconds (at most
//!   one second) on every block, like a heavy plug-in.
//! - `test:burst:<µs>`: an instrument that sounds a steady [`BURST_LEVEL`] on every block, and
//!   takes `µs` microseconds (at most one second, sleeping) over a block that brings notes or
//!   controllers, like a synthesizer that starts or resets its voices when playback
//!   starts or stops (#506).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Something that processes blocks. Shared between the control thread (state) and the audio
/// thread (process), so its state lives in atomics.
pub(crate) trait Processor: Send + Sync {
    fn process(&self, input: &[f32], output: &mut [f32]);
    /// [`Processor::process`] with the block's notes and controllers; most test processors
    /// ignore them.
    fn process_events(&self, input: &[f32], events: &[suhina_ipc::Event], output: &mut [f32]) {
        let _ = events;
        self.process(input, output);
    }
    fn latency(&self) -> u32 {
        0
    }
    fn state(&self) -> Vec<u8>;
    fn set_state(&self, data: &[u8]) -> Result<(), String>;
}

/// A test processor as an instance's audio processing.
pub(crate) struct TestBlock(pub(crate) std::sync::Arc<dyn Processor>);

impl crate::audio::Block for TestBlock {
    fn process(&mut self, input: &[f32], events: &[suhina_ipc::Event], output: &mut [f32]) {
        self.0.process_events(input, events, output);
    }
}

pub(crate) fn create(plugin: &str) -> Result<std::sync::Arc<dyn Processor>, String> {
    let after = |prefix: &str| plugin.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok());
    if plugin == "test:gain" {
        Ok(std::sync::Arc::new(Gain(AtomicU32::new(0.5f32.to_bits()))))
    } else if plugin == "test:transport-cut" {
        Ok(std::sync::Arc::new(TransportCut(AtomicU32::new(0))))
    } else if let Some(n) = after("test:crash-after:") {
        Ok(std::sync::Arc::new(Failing { after: n, blocks: AtomicU32::new(0), hang: false }))
    } else if let Some(ms) = after("test:slow-load:") {
        std::thread::sleep(Duration::from_millis(u64::from(ms.min(60_000))));
        Ok(std::sync::Arc::new(Gain(AtomicU32::new(0.5f32.to_bits()))))
    } else if let Some(n) = after("test:slow-start:") {
        Ok(std::sync::Arc::new(SlowStart { slow: n, blocks: AtomicU32::new(0) }))
    } else if let Some((n, frames)) = plugin.strip_prefix("test:delay-after:").and_then(|rest| rest.split_once(':')) {
        let (Ok(after), Ok(frames)) = (n.parse::<u32>(), frames.parse::<u32>()) else {
            return Err(format!("unknown plug-in {plugin:?}"));
        };
        let frames = frames.min(MAX_DELAY);
        Ok(std::sync::Arc::new(DelayAfter { after, frames, blocks: AtomicU32::new(0), line: Mutex::default() }))
    } else if let Some(micros) = after("test:busy:") {
        Ok(std::sync::Arc::new(Busy(Duration::from_micros(u64::from(micros.min(1_000_000))))))
    } else if let Some(micros) = after("test:burst:") {
        Ok(std::sync::Arc::new(Burst(Duration::from_micros(u64::from(micros.min(1_000_000))))))
    } else if let Some(n) = after("test:hang-after:") {
        Ok(std::sync::Arc::new(Failing { after: n, blocks: AtomicU32::new(0), hang: true }))
    } else {
        Err(format!("unknown plug-in {plugin:?}"))
    }
}

struct Gain(AtomicU32);

/// A low-frequency instrument that resets its output instantly when transport stops.
/// This models the discontinuity observed in an installed instrument without its preset.
struct TransportCut(AtomicU32);

impl Processor for TransportCut {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        self.process_events(input, &[], output);
    }
    fn process_events(&self, _input: &[f32], events: &[suhina_ipc::Event], output: &mut [f32]) {
        let playing =
            events.iter().any(|event| matches!(event.kind, suhina_ipc::EventKind::Transport { playing: true, .. }));
        let previous = self.0.load(Ordering::Relaxed);
        let mut phase = previous >> 2;
        let mut live = previous & 2 != 0;
        if previous & 1 != 0 && !playing {
            live = false;
        }
        for event in events {
            match event.kind {
                suhina_ipc::EventKind::NoteOn { .. } => live = true,
                suhina_ipc::EventKind::NoteOff { .. } => live = false,
                _ => {}
            }
        }
        for pair in output.as_chunks_mut::<2>().0 {
            let sample = if playing || live { (phase as f32 * std::f32::consts::TAU / 960.0).sin() * 0.5 } else { 0.0 };
            pair.fill(sample);
            phase = (phase + 1) % 960;
        }
        self.0.store((phase << 2) | u32::from(playing) | (u32::from(live) << 1), Ordering::Relaxed);
    }
    fn state(&self) -> Vec<u8> {
        Vec::new()
    }
    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

impl Processor for Gain {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        let gain = f32::from_bits(self.0.load(Ordering::Relaxed));
        for (o, i) in output.iter_mut().zip(input) {
            *o = i * gain;
        }
    }

    fn state(&self) -> Vec<u8> {
        f32::from_bits(self.0.load(Ordering::Relaxed)).to_le_bytes().to_vec()
    }

    fn set_state(&self, data: &[u8]) -> Result<(), String> {
        let bytes: [u8; 4] = data.try_into().map_err(|_| format!("gain state is 4 bytes, not {}", data.len()))?;
        let gain = f32::from_le_bytes(bytes);
        if !gain.is_finite() {
            return Err("gain is not a finite number".into());
        }
        self.0.store(gain.to_bits(), Ordering::Relaxed);
        Ok(())
    }
}

/// A `test:busy:<µs>` plug-in: halves the sound, keeping its thread busy for that long on every
/// block, as a heavy plug-in does (to measure how the engine waits for several, #151).
struct Busy(Duration);

impl Processor for Busy {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        let started = std::time::Instant::now();
        while started.elapsed() < self.0 {
            std::hint::spin_loop();
        }
        for (o, i) in output.iter_mut().zip(input) {
            *o = i * 0.5;
        }
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

/// What a `test:burst` plug-in adds to every sample, so a block it did not answer in time is
/// heard (and seen) as silence.
pub(crate) const BURST_LEVEL: f32 = 0.25;

/// A `test:burst:<µs>` plug-in: an instrument whose voices cost nothing while they hold, and
/// that takes that long over a block that brings notes or controllers (#506).
struct Burst(Duration);

impl Processor for Burst {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        self.process_events(input, &[], output);
    }

    fn process_events(&self, input: &[f32], events: &[suhina_ipc::Event], output: &mut [f32]) {
        // It sleeps rather than spins: the host's audio thread may run at real-time priority,
        // and spinning there could take the engine's core on a small machine, which would
        // measure the scheduler rather than the engine.
        // Notes and controllers start or reset voices; the song's place, which comes with every
        // block, does not.
        use suhina_ipc::EventKind;
        let voices = events.iter().any(|e| {
            matches!(
                e.kind,
                EventKind::NoteOn { .. }
                    | EventKind::NoteOff { .. }
                    | EventKind::Controller { .. }
                    | EventKind::PitchBend { .. }
            )
        });
        if voices {
            std::thread::sleep(self.0);
        }
        for (o, i) in output.iter_mut().zip(input) {
            *o = i + BURST_LEVEL;
        }
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

/// The time each of a `test:slow-start` plug-in's first blocks takes.
const SLOW_BLOCK: Duration = Duration::from_millis(30);

struct SlowStart {
    slow: u32,
    blocks: AtomicU32,
}

impl Processor for SlowStart {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        if self.blocks.fetch_add(1, Ordering::Relaxed) < self.slow {
            std::thread::sleep(SLOW_BLOCK);
        }
        for (o, i) in output.iter_mut().zip(input) {
            *o = i * 0.5;
        }
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

struct Failing {
    after: u32,
    blocks: AtomicU32,
    hang: bool,
}

impl Processor for Failing {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        if self.blocks.fetch_add(1, Ordering::Relaxed) >= self.after {
            if !self.hang {
                std::process::exit(3);
            }
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        output.copy_from_slice(input);
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

/// Longest delay a `test:delay-after` plug-in takes, in frames.
const MAX_DELAY: u32 = 96_000;

/// Channels a `test:delay-after` plug-in's blocks carry, interleaved.
const DELAY_CHANNELS: usize = 2;

struct DelayAfter {
    after: u32,
    frames: u32,
    blocks: AtomicU32,
    /// The samples on their way through, once it delays.
    line: Mutex<VecDeque<f32>>,
}

impl Processor for DelayAfter {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        let block = self.blocks.fetch_add(1, Ordering::AcqRel);
        if block < self.after {
            output.copy_from_slice(input);
            return;
        }
        let Ok(mut line) = self.line.lock() else {
            output.fill(0.0);
            return;
        };
        if block == self.after {
            line.resize(self.frames as usize * DELAY_CHANNELS, 0.0);
        }
        for (o, i) in output.iter_mut().zip(input) {
            line.push_back(*i);
            *o = line.pop_front().unwrap_or(0.0);
        }
    }

    fn latency(&self) -> u32 {
        if self.blocks.load(Ordering::Acquire) > self.after { self.frames } else { 0 }
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delay_after_plug_in_delays_from_its_block_on_and_says_so() {
        let delay = create("test:delay-after:1:2").unwrap();
        let mut output = [0.0f32; 8];
        delay.process(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], &mut output);
        assert_eq!(output, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(delay.latency(), 0);
        delay.process(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], &mut output);
        assert_eq!(output, [0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0], "two stereo frames late");
        assert_eq!(delay.latency(), 2);
        delay.process(&[0.0; 8], &mut output);
        assert_eq!(output, [5.0, 6.0, 7.0, 8.0, 0.0, 0.0, 0.0, 0.0]);
        for bad in ["test:delay-after:1", "test:delay-after:x:2", "test:delay-after:1:-2"] {
            assert!(create(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_busy_plug_in_takes_its_time_and_halves() {
        let busy = create("test:busy:2000").unwrap();
        let mut output = [0.0f32; 4];
        let started = std::time::Instant::now();
        busy.process(&[1.0, 2.0, 3.0, 4.0], &mut output);
        assert!(started.elapsed() >= Duration::from_millis(2));
        assert_eq!(output, [0.5, 1.0, 1.5, 2.0]);
        assert!(create("test:busy:x").is_err());
    }

    #[test]
    fn a_burst_plug_in_sounds_and_takes_its_time_only_over_events() {
        let burst = create("test:burst:5000").unwrap();
        let mut output = [0.0f32; 4];
        let started = std::time::Instant::now();
        burst.process_events(&[1.0, 0.0, 0.0, 0.0], &[], &mut output);
        assert!(started.elapsed() < Duration::from_millis(5));
        assert_eq!(output, [1.25, 0.25, 0.25, 0.25]);
        let started = std::time::Instant::now();
        burst.process_events(&[0.0; 4], &[suhina_ipc::Event::note_on(0, 0, 60, 1.0)], &mut output);
        assert!(started.elapsed() >= Duration::from_millis(5));
        assert!(create("test:burst:x").is_err());
    }
}
