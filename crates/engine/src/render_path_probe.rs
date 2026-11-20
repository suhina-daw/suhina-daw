// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opt-in, single-effect-chain laboratory for ADR 0038's path switch (#1508).
//!
//! The caller supplies source audio by frame; this does not play arrangement or launcher clips.
//! Devices use the ordinary chain code. Only their pre-fader output is queued; the mixer and
//! output compensation run when a block is consumed. No device is cloned, rewound or reset.
//! See `docs/spec/render-path-prototype.md` for the experiment's limits.

use std::fmt;

use suhina_device_api::ProcessContext;
use suhina_model::TrackId;

use crate::Renderer;
use crate::chain::{Chain, MAX_BLOCK};
use crate::plugin::{SongHistory, SongPosition};

/// Settings for one fixed-block experiment. A zero lead disables rendering ahead.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub block_frames: usize,
    /// 0 (off), 50, 100 or 200 ms, rounded up to whole blocks.
    pub lead_ms: u32,
}

/// The requested path; draining and catching up are reported separately in [`Status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    Live,
    Ahead,
}

/// The path currently heard, after a call to [`Probe::process`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Live,
    Draining,
    CatchingUp,
    Ahead,
}

/// Frame-domain observations, independent of the machine's speed or scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub played_frames: u64,
    pub rendered_frames: u64,
    pub queued_frames: usize,
    pub target_frames: usize,
    /// Device blocks processed by the most recent call, at most two.
    pub rendered_blocks: usize,
}

/// Invalid probe inputs. Errors are checked before processing or changing a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    InvalidSettings,
    AlreadyStarted,
    UnsupportedTrack,
    Disabled,
    BlockLength,
    PositionLimit,
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSettings => "the probe needs 1–1024 frames and a lead of 0, 50, 100 or 200 ms",
            Self::AlreadyStarted => "the probe needs a fresh renderer at frame zero",
            Self::UnsupportedTrack => "the probe needs a leaf effect chain without buses, sends, keys or ARA",
            Self::Disabled => "the probe's render-ahead lead is off",
            Self::BlockLength => "the probe output must contain exactly one configured stereo block",
            Self::PositionLimit => "the probe's frame counter is exhausted",
        })
    }
}

impl std::error::Error for ProbeError {}

/// A single prepared effect chain, a bounded pre-fader queue, and two frame cursors.
/// Construction allocates. Processing and controls do not allocate or free.
pub struct Probe {
    chain: Chain,
    songs: SongHistory,
    sample_rate: u32,
    tempo: f64,
    block_frames: usize,
    target_blocks: usize,
    queue: Vec<f32>,
    head: usize,
    queued: usize,
    played: u64,
    rendered: u64,
    path: Path,
    rendered_blocks: usize,
}

impl Probe {
    /// Extracts `track` from a fresh renderer at frame zero, preserving its devices and compensation.
    /// The other chains and the renderer are retired here, off the audio thread. Sources are
    /// supplied to [`Self::process`], not read from the renderer's project. The probe waits for
    /// plug-ins with the engine's existing offline timeout; it measures no real-time deadline.
    pub fn new(mut renderer: Renderer, track: TrackId, config: Config) -> Result<Self, ProbeError> {
        if !(1..=MAX_BLOCK).contains(&config.block_frames) || !matches!(config.lead_ms, 0 | 50 | 100 | 200) {
            return Err(ProbeError::InvalidSettings);
        }
        if renderer.transport.position != 0 {
            return Err(ProbeError::AlreadyStarted);
        }
        let index = renderer.graph.chains.iter().position(|chain| chain.track == track);
        let Some(index) = index else { return Err(ProbeError::UnsupportedTrack) };
        let Some(chain) = renderer.graph.chains.get(index) else { return Err(ProbeError::UnsupportedTrack) };
        if track == TrackId::MAIN
            || chain.instrument
            || chain.is_bus
            || chain.turn.feeders != 0
            || chain.target.is_some()
            || !chain.sends.is_empty()
            || !chain.key_reads.is_empty()
            || !chain.key_readers.is_empty()
            || chain.ara.is_some()
        {
            return Err(ProbeError::UnsupportedTrack);
        }
        let sample_rate = renderer.sample_rate();
        let tempo = renderer.graph.launcher.tempo_bpm();
        // Both factors are bounded: the renderer validates the rate and settings above cap
        // the lead. One spare slot holds a live block before it is consumed.
        let lead_frames = (u64::from(sample_rate) * u64::from(config.lead_ms)).div_ceil(1000);
        let target_blocks = lead_frames.div_ceil(config.block_frames as u64) as usize;
        let queue = vec![0.0; (target_blocks + 1) * config.block_frames * 2];
        let chain = renderer.graph.chains.remove(index);
        Ok(Self {
            chain,
            songs: renderer.songs,
            sample_rate,
            tempo,
            block_frames: config.block_frames,
            target_blocks,
            queue,
            head: 0,
            queued: 0,
            played: 0,
            rendered: 0,
            path: Path::Live,
            rendered_blocks: 0,
        })
    }

    /// Requests a path without discarding queued sound or changing device state.
    pub fn set_path(&mut self, path: Path) -> Result<(), ProbeError> {
        if path == Path::Ahead && self.target_blocks == 0 {
            return Err(ProbeError::Disabled);
        }
        self.path = path;
        Ok(())
    }

    /// A device parameter takes effect at the next frame not yet rendered.
    pub fn set_device_param(&mut self, device: usize, param: usize, value: f64) {
        if value.is_finite() {
            self.chain.set_param(device, param, value);
        }
    }

    /// A fader move takes effect on the next played block, with ordinary smoothing.
    pub fn set_volume(&mut self, volume: f64) {
        self.chain.set_live_volume(volume);
    }

    /// A pan move takes effect on the next played block, with ordinary smoothing.
    pub fn set_pan(&mut self, pan: f64) {
        self.chain.set_live_pan(pan);
    }

    /// The chain's device latency and its output compensation, unchanged by switching paths.
    pub fn latency(&self) -> (u32, usize) {
        (self.chain.latency, self.chain.delay.frames())
    }

    pub fn status(&self) -> Status {
        let state = match self.path {
            Path::Live if self.queued == 0 => State::Live,
            Path::Live => State::Draining,
            Path::Ahead if self.queued < self.target_blocks => State::CatchingUp,
            Path::Ahead => State::Ahead,
        };
        Status {
            state,
            played_frames: self.played,
            rendered_frames: self.rendered,
            queued_frames: self.queued * self.block_frames,
            target_frames: self.target_blocks * self.block_frames,
            rendered_blocks: self.rendered_blocks,
        }
    }

    /// Plays exactly one block. `source` fills audio for the given absolute source frame;
    /// it must itself be real-time safe. It is borrowed so its storage is not dropped here.
    /// With `catch_up`, at most two device blocks run;
    /// otherwise at most one runs, maintaining any existing lead without increasing it.
    /// No clock is read to guess a budget, and no work is dropped to meet one.
    pub fn process(
        &mut self,
        out: &mut [f32],
        catch_up: bool,
        source: &mut impl FnMut(u64, &mut [f32]),
    ) -> Result<Status, ProbeError> {
        if out.len() != self.block_frames * 2 {
            return Err(ProbeError::BlockLength);
        }
        let budget = if catch_up { 2 } else { 1 };
        let frames = self.block_frames as u64;
        if self.played.checked_add(frames).is_none() || self.rendered.checked_add(frames * budget).is_none() {
            return Err(ProbeError::PositionLimit);
        }
        self.rendered_blocks = 0;
        if self.queued == 0 {
            self.render(source);
        }
        let start = self.head * out.len();
        if let Some(block) = self.queue.get(start..start + out.len()) {
            self.chain.input(self.block_frames).copy_from_slice(block);
        }
        self.head = (self.head + 1) % (self.target_blocks + 1);
        self.queued -= 1;
        self.chain.automate_mix(self.played, self.block_frames, true);
        self.chain.plan_mix(self.block_frames);
        self.chain.keep_post(self.block_frames);
        // This is the same mixer and delay line as an ordinary chain, at play time.
        out.fill(0.0);
        self.chain.mix(out, &mut [], self.block_frames);
        self.played += frames;
        if self.path == Path::Ahead {
            while self.queued < self.target_blocks && self.rendered_blocks < budget as usize {
                self.render(source);
            }
        }
        Ok(self.status())
    }

    fn render(&mut self, source: &mut impl FnMut(u64, &mut [f32])) {
        let frames = self.block_frames;
        self.chain.begin(frames, None);
        source(self.rendered, self.chain.input(frames));
        let beat = self.rendered as f64 * self.tempo / (60.0 * f64::from(self.sample_rate));
        let context = ProcessContext { tempo: self.tempo, beat, playing: true };
        self.songs.record(
            SongPosition {
                playing: true,
                position: self.rendered,
                tempo: self.tempo,
                beat,
                bar: (beat / 4.0) as u32,
                bar_start: (beat / 4.0).floor() * 4.0,
                ..SongPosition::default()
            },
            frames,
        );
        self.chain.transport(&self.songs);
        self.chain.automate(&self.songs, frames);
        self.chain.run_devices(frames, &context);
        let slot = (self.head + self.queued) % (self.target_blocks + 1);
        let start = slot * frames * 2;
        if let Some(block) = self.queue.get_mut(start..start + frames * 2) {
            block.copy_from_slice(self.chain.output(frames));
        }
        self.queued += 1;
        self.rendered += frames as u64;
        self.rendered_blocks += 1;
    }
}

#[cfg(test)]
#[path = "render_path_probe_tests.rs"]
mod tests;
