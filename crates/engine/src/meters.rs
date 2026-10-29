// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Level meters (`docs/spec/level-meters.md`): the peak and RMS level of every track and of
//! the main output, per channel, with a clip indicator that stays lit until it is reset.
//!
//! The renderer measures every block and publishes to a [`MeterMonitor`], a fixed table of
//! atomics any thread reads, like [`crate::CpuMonitor`]: the audio thread only stores into it,
//! never allocates, locks or waits. A track is measured at its own level, after its own
//! devices, volume and pan and before the groups and the main channel it plays through; a
//! group shows the sum of what plays into it, after its own volume. A track that is not heard
//! (muted, or another track soloed) shows nothing. The main output is what the renderer hands
//! the audio device, preview and metronome included.
//!
//! [`Levels`] is the graph's half: built with the graph, off the audio thread, it holds a
//! block buffer per track that the track's sources add to as they are mixed. [`MeterWriter`] is
//! the renderer's half: the meter ballistics and the monitor it publishes to.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use suhina_model::{ChannelId, ChannelRole, Project, TrackId};

use crate::buses::gains_until;
use crate::chain::{CHAIN_INPUT_GAIN, Chain, MAX_BLOCK};
use crate::route;
use crate::spectrum;

/// Tracks that are metered; a project's tracks after these have no meter.
pub const MAX_METERED_TRACKS: usize = 512;
/// The meter slot of a source that is not metered (a track past `MAX_METERED_TRACKS`).
pub(crate) const NO_METER: u32 = u32::MAX;
/// How fast a peak falls once the sound is quieter, in dB per second.
pub const PEAK_FALL_DB_PER_SECOND: f64 = 24.0;
/// The time constant the mean square behind the RMS level is averaged over, in seconds.
pub const RMS_SECONDS: f64 = 0.3;
/// Levels below this (−120 dB) are silence: they read as zero, so a decaying meter ends.
const FLOOR: f32 = 1e-6;
/// The track id of an unused row.
const NO_TRACK: u32 = u32::MAX;

/// One meter's level as last published: per channel (left, right), linear (1.0 is 0 dBFS).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    /// The highest sample, falling at [`PEAK_FALL_DB_PER_SECOND`] once the sound is quieter.
    pub peak: [f32; 2],
    /// The RMS level averaged over about [`RMS_SECONDS`].
    pub rms: [f32; 2],
    /// A sample went over full scale (0 dBFS) since the indicator was last reset.
    pub clipped: bool,
}

#[derive(Debug, Default)]
struct Row {
    track: AtomicU32,
    peak: [AtomicU32; 2],
    rms: [AtomicU32; 2],
    clipped: AtomicBool,
    sample: std::sync::atomic::AtomicU64,
    voices: [AtomicU32; 32],
}

impl Row {
    fn level(&self) -> Level {
        let read = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        Level {
            peak: [read(&self.peak[0]), read(&self.peak[1])],
            rms: [read(&self.rms[0]), read(&self.rms[1])],
            clipped: self.clipped.load(Ordering::Relaxed),
        }
    }

    fn store(&self, peak: [f32; 2], rms: [f32; 2]) {
        for (a, v) in self.peak.iter().zip(peak) {
            a.store(v.to_bits(), Ordering::Relaxed);
        }
        for (a, v) in self.rms.iter().zip(rms) {
            a.store(v.to_bits(), Ordering::Relaxed);
        }
    }
}

/// The levels of the main output and of every metered track, written by the audio thread
/// and readable from any thread. Clip indicators are reset from any thread. It also carries
/// the watched devices' output rings the EQ panel's analyser reads
/// ([`spectrum::Spectrum`], docs/spec/eq.md › The panel › The analyser).
#[derive(Debug)]
pub struct MeterMonitor {
    main: Row,
    tracks: Box<[Row]>,
    /// Shared with the graph's chains, which capture their watched devices' output into it on
    /// every path that runs them.
    pub(crate) spectrum: std::sync::Arc<spectrum::Spectrum>,
}

impl Default for MeterMonitor {
    fn default() -> Self {
        let tracks: Box<[Row]> =
            (0..MAX_METERED_TRACKS).map(|_| Row { track: AtomicU32::new(NO_TRACK), ..Row::default() }).collect();
        Self { main: Row::default(), tracks, spectrum: std::sync::Arc::new(spectrum::Spectrum::new()) }
    }
}

impl MeterMonitor {
    /// Active sample read head for a channel device.
    pub fn sample_position(&self, track: TrackId, device: usize) -> Option<f32> {
        let packed = self.row(track)?.sample.load(Ordering::Relaxed);
        if packed >> 32 != u64::try_from(device).ok()?.checked_add(1)? {
            return None;
        }
        Some(f32::from_bits(packed as u32))
    }
    /// Independent active read heads for the channel's sample instrument.
    pub fn sample_positions(&self, track: TrackId, device: usize) -> [Option<f32>; 32] {
        let Some(row) = self.row(track) else { return [None; 32] };
        if row.sample.load(Ordering::Acquire) >> 32
            != u64::try_from(device).ok().and_then(|v| v.checked_add(1)).unwrap_or(0)
        {
            return [None; 32];
        }
        std::array::from_fn(|i| {
            let value = f32::from_bits(row.voices[i].load(Ordering::Relaxed));
            (value >= 0.).then_some(value)
        })
    }
    /// The main output's level.
    pub fn main(&self) -> Level {
        self.main.level()
    }

    /// The watched devices' output rings, the EQ panel's analyser's source
    /// (docs/spec/eq.md › The panel › The analyser).
    pub fn spectrum(&self) -> &spectrum::Spectrum {
        &self.spectrum
    }

    /// The level of `track`, or `None` when the engine plays no such track or it has no meter.
    pub fn track(&self, track: TrackId) -> Option<Level> {
        self.row(track).map(Row::level)
    }

    /// Every metered track's level, in the engine's track order.
    pub fn tracks(&self) -> Vec<(TrackId, Level)> {
        self.tracks
            .iter()
            .filter_map(|row| match row.track.load(Ordering::Relaxed) {
                NO_TRACK => None,
                id => Some((TrackId(id), row.level())),
            })
            .collect()
    }

    /// Turns off the clip indicator of `track`; false when it has no meter.
    pub fn reset_clip(&self, track: TrackId) -> bool {
        self.row(track).map(|row| row.clipped.store(false, Ordering::Relaxed)).is_some()
    }

    /// Turns off the main output's clip indicator.
    pub fn reset_main_clip(&self) {
        self.main.clipped.store(false, Ordering::Relaxed);
    }

    /// Turns off every clip indicator, the main output's too.
    pub fn reset_all_clips(&self) {
        self.reset_main_clip();
        for row in self.tracks.iter() {
            row.clipped.store(false, Ordering::Relaxed);
        }
    }

    fn row(&self, track: TrackId) -> Option<&Row> {
        self.tracks.iter().find(|row| row.track.load(Ordering::Relaxed) == track.0)
    }
}

/// The meter slot of `track`: its index among the project's tracks, or [`NO_METER`].
pub(crate) fn meter_slot(project: &Project, track: TrackId) -> u32 {
    project
        .tracks
        .iter()
        .position(|t| t.id == track)
        .filter(|&i| i < MAX_METERED_TRACKS)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(NO_METER)
}

/// The graph's half of the meters: a block buffer per metered track that its sources add to,
/// and what turns a source's mixed sound into the track's own level. Allocates when built:
/// build it off the audio thread, with the graph.
pub(crate) struct Levels {
    /// The track of each slot, in the project's order.
    tracks: Vec<TrackId>,
    /// Per slot, the factor from a source's sound as mixed into the output (at the track's
    /// gains through every group and the main channel) to the track's own level.
    direct: Vec<(f32, f32)>,
    /// Per slot, the factor from the track chain's output, before its gains, to its own level.
    chained: Vec<(f32, f32)>,
    /// Per slot, the group slot it plays into whose level is the sum of what plays into it,
    /// and the group's volume.
    parent: Vec<Option<(usize, f32)>>,
    /// Slots in the order their sums are passed on: every slot before the group it plays into.
    order: Vec<usize>,
    /// Per chain, its track's slot.
    chain_slots: Vec<Option<usize>>,
    /// `MAX_BLOCK` interleaved stereo frames per slot.
    buffer: Vec<f32>,
    /// Per slot, whether anything was added to it this block.
    touched: Vec<bool>,
}

impl Levels {
    /// The meters of `project`'s tracks as the graph with `chains` plays them.
    pub(crate) fn build(project: &Project, chains: &[Chain]) -> Self {
        let tracks: Vec<TrackId> = project.tracks.iter().take(MAX_METERED_TRACKS).map(|t| t.id).collect();
        let channel_of = |track: TrackId| {
            let id = project.track(track)?.channel?;
            project.channel(id)
        };
        let master = project.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| c.id);
        let is_group =
            |channel: ChannelId| project.channels.iter().any(|c| c.id != channel && c.output == Some(channel));
        // A track's own gains: its volume and pan (a group's pan does not apply, as when it is
        // mixed), up to the next channel it plays into.
        let own = |track: TrackId| -> (f32, f32) {
            let Some(channel) = channel_of(track) else { return (0.0, 0.0) };
            let until = channel.output.or(master.filter(|&m| m != channel.id));
            gains_until(project, track, until, !is_group(channel.id)).unwrap_or((0.0, 0.0))
        };
        let ratio = |a: f32, b: f32| if b > 0.0 && b.is_finite() { a / b } else { 0.0 };
        let mut direct = Vec::with_capacity(tracks.len());
        let mut chained = Vec::with_capacity(tracks.len());
        for &track in &tracks {
            let own = own(track);
            let mixed = match route(project, chains, track) {
                Some((0, gains)) => gains,
                _ => (0.0, 0.0),
            };
            direct.push((ratio(own.0, mixed.0), ratio(own.1, mixed.1)));
            chained.push((own.0 / CHAIN_INPUT_GAIN, own.1 / CHAIN_INPUT_GAIN));
        }
        // A group whose chain the tracks inside it play into (a bus) hears them through it.
        let is_bus = |track: TrackId| chains.iter().any(|c| c.track == track && c.is_bus);
        let parent: Vec<Option<(usize, f32)>> = tracks
            .iter()
            .map(|&track| {
                let output = channel_of(track)?.output?;
                let group = project.tracks.iter().position(|t| t.channel == Some(output))?;
                let group_track = tracks.get(group).copied()?;
                if is_bus(group_track) {
                    return None;
                }
                let volume = project.channel(output).map_or(0.0, |c| c.volume);
                let volume = if volume.is_finite() { volume.max(0.0) as f32 } else { 0.0 };
                Some((group, volume))
            })
            .collect();
        // Deepest first, so a group has every track inside it before it passes its sum on.
        let depth = |slot: usize| {
            let mut depth = 0;
            let mut next = parent.get(slot).copied().flatten();
            while let Some((up, _)) = next {
                depth += 1;
                if depth > tracks.len() {
                    break;
                }
                next = parent.get(up).copied().flatten();
            }
            depth
        };
        let mut order: Vec<usize> = (0..tracks.len()).collect();
        order.sort_by_key(|&slot| std::cmp::Reverse(depth(slot)));
        let chain_slots = chains.iter().map(|c| tracks.iter().position(|&t| t == c.track)).collect();
        Self {
            buffer: vec![0.0; tracks.len() * MAX_BLOCK * 2],
            touched: vec![false; tracks.len()],
            tracks,
            direct,
            chained,
            parent,
            order,
            chain_slots,
        }
    }

    /// The track of every slot.
    pub(crate) fn tracks(&self) -> &[TrackId] {
        &self.tracks
    }

    /// Adds a frame of sound that a source of slot `meter` mixed into the output at `frame` of
    /// the block. Never allocates.
    pub(crate) fn add(&mut self, meter: u32, frame: usize, left: f32, right: f32) {
        let Ok(slot) = usize::try_from(meter) else { return };
        let Some(&(gain_left, gain_right)) = self.direct.get(slot) else { return };
        let at = (slot * MAX_BLOCK + frame) * 2;
        if let Some([l, r]) = self.buffer.get_mut(at..at + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok()) {
            *l += left * gain_left;
            *r += right * gain_right;
            if let Some(touched) = self.touched.get_mut(slot) {
                *touched = true;
            }
        }
    }

    /// Measures the block of `frames` frames: adds every chain's output to its track, passes
    /// each group's tracks on to it, publishes every track to `writer` and clears the buffers
    /// for the next block. Call it once the chains have finished the block. Never allocates.
    pub(crate) fn finish(&mut self, chains: &[Chain], frames: usize, writer: &mut MeterWriter) {
        let frames = frames.min(MAX_BLOCK);
        for (chain, slot) in chains.iter().zip(&self.chain_slots) {
            if let Some(row) = slot.and_then(|slot| writer.monitor.tracks.get(slot)) {
                let heads = chain.sample_positions();
                let positions = heads.as_ref().map_or([None; 32], |(_, positions)| *positions);
                for (slot, value) in row.voices.iter().zip(positions) {
                    slot.store(value.unwrap_or(-1.).to_bits(), Ordering::Relaxed);
                }
                let packed = heads
                    .and_then(|(device, positions)| {
                        let device = u32::try_from(device).ok()?.checked_add(1)?;
                        let first = positions.into_iter().flatten().next()?;
                        Some((u64::from(device) << 32) | u64::from(first.to_bits()))
                    })
                    .unwrap_or(0);
                row.sample.store(packed, Ordering::Release);
            }
        }
        for (chain, slot) in chains.iter().zip(&self.chain_slots) {
            let Some(slot) = *slot else { continue };
            let Some(&(gain_left, gain_right)) = self.chained.get(slot) else { continue };
            let at = slot * MAX_BLOCK * 2;
            let Some(to) = self.buffer.get_mut(at..at + frames * 2) else { continue };
            for ([l, r], [cl, cr]) in to.as_chunks_mut::<2>().0.iter_mut().zip(chain.output(frames).as_chunks::<2>().0)
            {
                *l += cl * gain_left;
                *r += cr * gain_right;
            }
            if let Some(touched) = self.touched.get_mut(slot) {
                *touched = true;
            }
        }
        for &slot in &self.order {
            let Some(Some((group, volume))) = self.parent.get(slot).copied() else { continue };
            if !self.touched.get(slot).copied().unwrap_or(false) {
                continue;
            }
            for i in 0..frames * 2 {
                let sample = self.buffer.get(slot * MAX_BLOCK * 2 + i).copied().unwrap_or(0.0);
                if let Some(to) = self.buffer.get_mut(group * MAX_BLOCK * 2 + i) {
                    *to += sample * volume;
                }
            }
            if let Some(touched) = self.touched.get_mut(group) {
                *touched = true;
            }
        }
        for slot in 0..self.tracks.len() {
            let touched = self.touched.get_mut(slot).map(std::mem::take).unwrap_or(false);
            if !touched {
                writer.measure(Meter::Track(slot), &[], frames);
                continue;
            }
            let at = slot * MAX_BLOCK * 2;
            if let Some(block) = self.buffer.get_mut(at..at + frames * 2) {
                writer.measure(Meter::Track(slot), block, frames);
                block.fill(0.0);
            }
        }
    }
}

/// Which meter a block is measured for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Meter {
    Main,
    Track(usize),
}

/// One meter's ballistics, per channel.
#[derive(Debug, Clone, Copy, Default)]
struct Ballistics {
    peak: [f32; 2],
    /// The averaged mean square.
    mean_square: [f32; 2],
}

/// The renderer's half of the meters: the ballistics of every meter and the monitor it
/// publishes them to. Owned by the renderer; nothing else may write to the monitor's levels.
pub(crate) struct MeterWriter {
    pub(crate) monitor: Arc<MeterMonitor>,
    sample_rate: f64,
    main: Ballistics,
    tracks: Vec<Ballistics>,
    /// The block length the falls below were worked out for, and the factors themselves:
    /// how much a peak keeps and how far the mean square moves towards the block's.
    frames: usize,
    peak_keep: f32,
    rms_move: f32,
}

impl MeterWriter {
    /// Allocates: call it off the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let monitor = Arc::new(MeterMonitor::default());
        monitor.spectrum.set_rate(sample_rate);
        Self {
            monitor,
            sample_rate: f64::from(sample_rate.max(1)),
            main: Ballistics::default(),
            tracks: vec![Ballistics::default(); MAX_METERED_TRACKS],
            frames: 0,
            peak_keep: 0.0,
            rms_move: 1.0,
        }
    }

    /// Shows the tracks of a new graph, slot by slot. A slot that now meters another track
    /// starts from silence with its clip indicator off, and no device is watched any more:
    /// the panels claim their rows again next frame. Never allocates.
    pub(crate) fn assign(&mut self, tracks: &[TrackId]) {
        self.monitor.spectrum.clear();
        for (slot, row) in self.monitor.tracks.iter().enumerate() {
            row.sample.store(0, Ordering::Relaxed);
            let id = tracks.get(slot).map_or(NO_TRACK, |t| t.0);
            if row.track.load(Ordering::Relaxed) == id {
                continue;
            }
            row.track.store(id, Ordering::Relaxed);
            row.store([0.0; 2], [0.0; 2]);
            row.clipped.store(false, Ordering::Relaxed);
            if let Some(state) = self.tracks.get_mut(slot) {
                *state = Ballistics::default();
            }
        }
    }

    /// Measures `block` (interleaved stereo; empty for silence) of `frames` frames for `meter`
    /// and publishes the result. Never allocates.
    pub(crate) fn measure(&mut self, meter: Meter, block: &[f32], frames: usize) {
        if frames == 0 {
            return;
        }
        if frames != self.frames {
            let seconds = frames as f64 / self.sample_rate;
            self.frames = frames;
            self.peak_keep = 10f64.powf(-PEAK_FALL_DB_PER_SECOND * seconds / 20.0) as f32;
            self.rms_move = (1.0 - (-seconds / RMS_SECONDS).exp()) as f32;
        }
        let mut peak = [0.0f32; 2];
        let mut sum = [0.0f64; 2];
        for frame in block.as_chunks::<2>().0 {
            for (side, &sample) in frame.iter().enumerate() {
                let magnitude = if sample.is_finite() { sample.abs() } else { 0.0 };
                if let (Some(p), Some(s)) = (peak.get_mut(side), sum.get_mut(side)) {
                    *p = p.max(magnitude);
                    *s += f64::from(magnitude) * f64::from(magnitude);
                }
            }
        }
        let (state, row) = match meter {
            Meter::Main => (&mut self.main, &self.monitor.main),
            Meter::Track(slot) => match (self.tracks.get_mut(slot), self.monitor.tracks.get(slot)) {
                (Some(state), Some(row)) => (state, row),
                _ => return,
            },
        };
        let mut rms = [0.0f32; 2];
        for side in 0..2 {
            let (Some(p), Some(m), Some(&block_peak), Some(&block_sum), Some(r)) = (
                state.peak.get_mut(side),
                state.mean_square.get_mut(side),
                peak.get(side),
                sum.get(side),
                rms.get_mut(side),
            ) else {
                continue;
            };
            *p = block_peak.max(*p * self.peak_keep);
            if *p < FLOOR {
                *p = 0.0;
            }
            let block_mean = (block_sum / frames as f64) as f32;
            *m += (block_mean - *m) * self.rms_move;
            if *m < FLOOR * FLOOR {
                *m = 0.0;
            }
            // The average falls more slowly than the peak once the sound stops; the RMS level of
            // a sound is never above its peak.
            *r = m.sqrt().min(*p);
        }
        row.store(state.peak, rms);
        if peak.iter().any(|&p| p > 1.0) {
            row.clipped.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod sample_position_tests {
    use super::*;
    #[test]
    fn voice_positions_remain_independent_and_inactive_slots_are_absent() {
        let mut writer = MeterWriter::new(48000);
        writer.assign(&[TrackId(7)]);
        let row = writer.monitor.row(TrackId(7)).unwrap();
        for slot in &row.voices {
            slot.store((-1f32).to_bits(), Ordering::Relaxed);
        }
        row.voices[0].store(0.25f32.to_bits(), Ordering::Relaxed);
        row.voices[3].store(0.75f32.to_bits(), Ordering::Relaxed);
        row.sample.store((3u64 << 32) | u64::from(0.25f32.to_bits()), Ordering::Release);
        let positions = writer.monitor.sample_positions(TrackId(7), 2);
        assert_eq!(positions[0], Some(0.25));
        assert_eq!(positions[3], Some(0.75));
        assert_eq!(positions.into_iter().flatten().count(), 2);
        assert_eq!(writer.monitor.sample_positions(TrackId(7), 1), [None; 32]);
        row.sample.store(0, Ordering::Release);
        assert_eq!(writer.monitor.sample_positions(TrackId(7), 2), [None; 32]);
    }

    #[test]
    fn read_head_is_scoped_to_track_and_device_and_cleared_on_graph_swap() {
        let mut writer = MeterWriter::new(48000);
        writer.assign(&[TrackId(7)]);
        let row = writer.monitor.row(TrackId(7)).unwrap();
        row.sample.store((3u64 << 32) | u64::from(0.25f32.to_bits()), Ordering::Relaxed);
        assert_eq!(writer.monitor.sample_position(TrackId(7), 2), Some(0.25));
        assert_eq!(writer.monitor.sample_position(TrackId(7), 1), None);
        assert_eq!(writer.monitor.sample_position(TrackId(8), 2), None);
        writer.assign(&[TrackId(7)]);
        assert_eq!(writer.monitor.sample_position(TrackId(7), 2), None);
    }
}
