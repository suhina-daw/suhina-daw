// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Real-time time-stretching of warped audio clips (ADR 0003).
//!
//! Each warped clip placement gets a [`StretchVoice`]: a [`suhina_stretch::Stretch`] built with
//! the renderer and fed from the decoded file in memory. Its playlist says which part of the file
//! plays when and how fast (warp marker segments, loop jumps). The stretcher reads the playlist
//! as one continuous input stream; each call covers one stretch of constant speed, and the speed
//! is the ratio of the input it is given to the output it makes. Nothing here allocates once the
//! voice is prepared (AGENTS.md rule 3); preparing it, which builds the stretcher, runs off the
//! audio thread.
//!
//! The stretcher runs at the file's sample rate, so pitch is kept against the file's own pitch;
//! its output is then read at the project's sample rate with linear interpolation. Transpose is
//! the stretcher's own pitch shift.

use std::sync::Arc;

use suhina_model::{AudioRegion, Clip, StretchMode, TempoMap};
use suhina_stretch::Stretch;

use crate::audio::{AudioData, STRETCH_SAMPLE_RATES, seconds_at, warp_markers};

/// Stretcher frames made per call.
const CHUNK_FRAMES: usize = 1024;
/// Speeds this close to 1 play without a stretcher (when not transposed).
const UNITY_TOLERANCE: f64 = 1e-4;
/// Speeds the stretcher is given; a clip with a segment outside them is re-pitched.
const RATES: std::ops::RangeInclusive<f64> = 0.25..=4.0;
/// Longest piece during a tempo ramp, in beats: a sixteenth note. The stretcher plays one speed a
/// piece, so a ramp is played as short steps, each starting at its exact time
/// (`docs/spec/tempo-map.md` › What follows the map).
const RAMP_PIECE_BEATS: f64 = 0.25;
/// Most playlist pieces per clip, so a malformed clip cannot exhaust memory. A looping clip that
/// needs more is cut after its last whole loop, and the voice repeats the loops after the first
/// loop start ([`Repeat`]), so it keeps looping.
const MAX_PIECES: usize = 100_000;

/// A stretch of the playlist: from `out_start` seconds after the clip starts, play file frames
/// `src_start..src_end` at `rate` times their own speed. `stream_start` is where the piece begins
/// in the stretcher's input stream (all pieces' file frames, one after another).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    out_start: f64,
    src_start: f64,
    src_end: f64,
    rate: f64,
    stream_start: f64,
}

/// Where a playlist cut at [`MAX_PIECES`] repeats: from piece `first` to the end the pieces cover
/// whole loops, `out` seconds of output and `stream` frames of input, played again and again.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Repeat {
    first: usize,
    out: f64,
    stream: f64,
}

/// A position in a playlist that may repeat: piece `index`, in repeat `turn` (0 for the pieces as
/// listed).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Place {
    index: usize,
    turn: u64,
}

/// When a clip's beats play: seconds after the clip's start.
enum Clock<'a> {
    /// At one tempo, in bpm.
    Constant(f64),
    /// Through a tempo map counted in frames at `sample_rate`, for a clip that starts at song
    /// beat `start`.
    Map { map: &'a TempoMap, start: f64, sample_rate: f64 },
}

impl Clock<'_> {
    /// Seconds from the clip's start to `played` beats into it.
    fn seconds(&self, played: f64) -> f64 {
        match self {
            Self::Constant(bpm) => played * (60.0 / bpm),
            Self::Map { map, start, sample_rate } => (map.time_at(start + played) - map.time_at(*start)) / sample_rate,
        }
    }

    /// The most beats a piece starting `played` beats into the clip may last: up to the next
    /// tempo change, and a sixteenth during a ramp. Unlimited at a constant tempo.
    fn piece_limit(&self, played: f64) -> f64 {
        let Self::Map { map, start, .. } = self else { return f64::INFINITY };
        let song = start + played;
        let next_change = map.next_break_after(song + 1e-9).map_or(f64::INFINITY, |b| b - song);
        let ramp = if map.ramps_at(song + 1e-9) { RAMP_PIECE_BEATS } else { f64::INFINITY };
        next_change.min(ramp)
    }
}

/// Works out which part of the file plays when, from the clip's offset, loop and length and the
/// region's warp markers at `tempo_bpm`. `None` when the clip cannot be stretched (fewer than two
/// usable warp markers, a speed that is not a finite positive number, or more than `MAX_PIECES`
/// pieces without a whole loop to repeat).
#[cfg(test)]
fn playlist(
    clip: &Clip,
    region: &AudioRegion,
    data: &AudioData,
    tempo_bpm: f64,
) -> Option<(Vec<Piece>, f64, Option<Repeat>)> {
    if !(tempo_bpm.is_finite() && tempo_bpm > 0.0) {
        return None;
    }
    playlist_on(clip, region, data, &Clock::Constant(tempo_bpm))
}

/// The playlist on any [`Clock`]: at a constant tempo the pieces are exactly the ones it has
/// always made; through a tempo map they are also cut at every tempo change and in a ramp.
fn playlist_on(
    clip: &Clip,
    region: &AudioRegion,
    data: &AudioData,
    clock: &Clock,
) -> Option<(Vec<Piece>, f64, Option<Repeat>)> {
    let markers = warp_markers(region);
    if markers.len() < 2 || markers.windows(2).any(|w| matches!(w, [a, b] if b.1 <= a.1)) {
        return None;
    }
    if let Clock::Constant(bpm) = clock
        && !(bpm.is_finite() && *bpm > 0.0)
    {
        return None;
    }
    if !(clip.length.is_finite() && clip.length > 0.0) {
        return None;
    }
    let file_rate = f64::from(data.sample_rate);
    let loop_range = clip.loop_range.filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start);

    let mut pieces = Vec::new();
    let mut beat = crate::audio::start_beat(clip.offset, loop_range.map(|r| (r.start, r.end)));
    let mut played = 0.0;
    let mut stream = 0.0;
    let mut max_rate: f64 = 0.0;
    // Where each pass of the loop began (piece, output beats, input frames): the first and the
    // latest, for a cut playlist.
    let mut first_loop: Option<(usize, f64, f64)> = None;
    let mut last_loop: Option<(usize, f64, f64)> = None;
    while played < clip.length - 1e-9 && pieces.len() < MAX_PIECES {
        let next_marker = markers.get(markers.partition_point(|m| m.0 <= beat + 1e-9)).map(|m| m.0);
        let loop_end = loop_range.map(|r| r.end).filter(|e| *e > beat + 1e-9);
        let clip_end = beat + (clip.length - played);
        let tempo_cut = Some(beat + clock.piece_limit(played)).filter(|b| b.is_finite());
        let next =
            [next_marker, loop_end, Some(clip_end), tempo_cut].into_iter().flatten().fold(f64::INFINITY, f64::min);
        let src_start = seconds_at(&markers, beat) * file_rate;
        let src_end = seconds_at(&markers, next) * file_rate;
        let out_seconds = match clock {
            Clock::Constant(bpm) => (next - beat) * (60.0 / bpm),
            Clock::Map { .. } => clock.seconds(played + (next - beat)) - clock.seconds(played),
        };
        let rate = (src_end - src_start) / file_rate / out_seconds;
        if !(rate.is_finite() && rate > 0.0) {
            return None;
        }
        max_rate = max_rate.max(rate).max(1.0 / rate);
        pieces.push(Piece { out_start: clock.seconds(played), src_start, src_end, rate, stream_start: stream });
        stream += src_end - src_start;
        played += next - beat;
        beat = next;
        if let Some(r) = loop_range
            && beat >= r.end - 1e-9
        {
            beat = r.start;
            let here = (pieces.len(), played, stream);
            first_loop.get_or_insert(here);
            last_loop = Some(here);
        }
    }
    if played < clip.length - 1e-9 {
        // Cut at the cap: keep whole loops and repeat them, or refuse the clip.
        let (Some(first), Some(last)) = (first_loop, last_loop) else { return None };
        let out = match clock {
            Clock::Constant(bpm) => (last.1 - first.1) * (60.0 / bpm),
            Clock::Map { .. } => clock.seconds(last.1) - clock.seconds(first.1),
        };
        let repeat = Repeat { first: first.0, out, stream: last.2 - first.2 };
        if last.0 <= first.0 || !(repeat.out > 0.0 && repeat.stream > 0.0) {
            return None;
        }
        pieces.truncate(last.0);
        return Some((pieces, max_rate, Some(repeat)));
    }
    Some((pieces, max_rate, None))
}

/// What a [`StretchVoice`]'s stretcher and buffers are built from ([`StretchVoice::spec`]): a
/// worker builds them from this while the voice stays where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StretchSpec {
    channels: usize,
    file_rate: f64,
    pitch: f64,
    max_rate: f64,
    step: f64,
}

impl StretchSpec {
    /// Builds the stretcher and the buffers. Allocates (1 to 4 ms for a mono clip at 48 kHz in a
    /// release build): call it off the audio thread.
    pub(crate) fn build(self) -> StretchParts {
        let channels = self.channels;
        let mut stretch = Stretch::new(channels, self.file_rate as f32);
        if self.pitch != 1.0 {
            stretch.set_transpose_factor(self.pitch as f32, 0.0);
        }
        // Input for one chunk at the fastest speed, or for a seek (which wants up to one block
        // plus the output latency at that speed).
        let input_frames = (CHUNK_FRAMES as f64 * self.max_rate).ceil() as usize
            + stretch.block_samples() * 2
            + (self.max_rate * stretch.output_latency() as f64).ceil() as usize
            + 2;
        // Room for one project block of up to CHUNK_FRAMES frames at any step, plus a chunk.
        let fifo_frames = (CHUNK_FRAMES as f64 * self.step.max(1.0)) as usize + 2 * CHUNK_FRAMES + 4;
        StretchParts {
            stretch,
            fifo: vec![0.0; fifo_frames * channels],
            input: vec![0.0; input_frames * channels],
            chunk: vec![0.0; CHUNK_FRAMES * channels],
        }
    }
}

/// A voice's stretcher and buffers, built by [`StretchSpec::build`] for
/// [`StretchVoice::install`].
pub(crate) struct StretchParts {
    stretch: Stretch,
    fifo: Vec<f32>,
    input: Vec<f32>,
    chunk: Vec<f32>,
}

/// One warped clip placement being stretched in real time.
pub(crate) struct StretchVoice {
    data: Arc<AudioData>,
    pieces: Vec<Piece>,
    repeat: Option<Repeat>,
    /// Built with its buffers before the voice first plays ([`StretchVoice::prepare`], or
    /// [`StretchSpec::build`] on another thread and [`StretchVoice::install`]), off the audio
    /// thread: building every voice of a project with hundreds of stretched clips for each edit
    /// held the window up for most of a second.
    stretch: Option<Stretch>,
    /// Moved since it was built: it plays from where the last seek put it.
    started: bool,
    /// The pitch factor and the fastest speed of the playlist, for that build.
    pitch: f64,
    max_rate: f64,
    channels: usize,
    /// Stretcher output frames per second of the clip (the file's sample rate).
    file_rate: f64,
    /// Input-stream frames given to the stretcher so far.
    fed: f64,
    /// Stretcher frames produced so far: the voice's own clock, at the file's sample rate.
    produced: u64,
    /// Stretched audio at the file's rate, waiting to be read; `fifo_read` is the next unread
    /// frame and `fifo_position` the fractional read position within it.
    fifo: Vec<f32>,
    fifo_len: usize,
    fifo_read: usize,
    fifo_position: f64,
    /// File frames per project frame.
    step: f64,
    input: Vec<f32>,
    chunk: Vec<f32>,
}

impl StretchVoice {
    /// Builds the voice for one clip placement, ready to play from the clip's start. Allocates;
    /// call it off the audio thread.
    pub(crate) fn new(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        tempo_bpm: f64,
        sample_rate: u32,
    ) -> Option<Self> {
        if !(tempo_bpm.is_finite() && tempo_bpm > 0.0) {
            return None;
        }
        Self::with_clock(clip, region, data, &Clock::Constant(tempo_bpm), sample_rate)
    }

    /// [`StretchVoice::new`] for a clip on an arrangement whose tempo follows `map` (counted in
    /// frames at `sample_rate`): its speed changes at every tempo change and through ramps, so
    /// it stays on the grid. At a constant tempo exactly [`StretchVoice::new`].
    pub(crate) fn on_map(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        map: &TempoMap,
        sample_rate: u32,
    ) -> Option<Self> {
        if map.is_constant() {
            return Self::new(clip, region, data, map.start_bpm(), sample_rate);
        }
        let clock = Clock::Map { map, start: clip.start, sample_rate: f64::from(sample_rate) };
        Self::with_clock(clip, region, data, &clock, sample_rate)
    }

    fn with_clock(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        clock: &Clock,
        sample_rate: u32,
    ) -> Option<Self> {
        let channels = usize::from(data.channels);
        if !(1..=8).contains(&channels) || data.frames() == 0 || !STRETCH_SAMPLE_RATES.contains(&data.sample_rate) {
            return None;
        }
        if region.stretch == StretchMode::Repitch {
            return None;
        }
        let (pieces, max_rate, repeat) = playlist_on(clip, region, &data, clock)?;
        let pitch = 2f64.powf(region.transpose / 12.0);
        let pitch = if pitch.is_finite() && pitch > 0.0 { pitch } else { 1.0 };
        // Audio that already plays at its own speed and pitch needs no stretcher: the caller reads
        // it directly.
        if pitch == 1.0 && pieces.iter().all(|p| (p.rate - 1.0).abs() <= UNITY_TOLERANCE) {
            return None;
        }
        if !pieces.iter().all(|p| RATES.contains(&p.rate)) {
            return None;
        }
        let file_rate = f64::from(data.sample_rate);
        let step = file_rate / f64::from(sample_rate.max(1));
        Some(Self {
            data,
            pieces,
            repeat,
            stretch: None,
            started: false,
            pitch,
            max_rate,
            channels,
            file_rate,
            fed: 0.0,
            produced: 0,
            fifo: Vec::new(),
            fifo_len: 0,
            fifo_read: 0,
            fifo_position: 0.0,
            step,
            input: Vec::new(),
            chunk: Vec::new(),
        })
    }

    /// What the voice's stretcher and buffers are built from, to build them on another thread.
    pub(crate) fn spec(&self) -> StretchSpec {
        StretchSpec {
            channels: self.channels,
            file_rate: self.file_rate,
            pitch: self.pitch,
            max_rate: self.max_rate,
            step: self.step,
        }
    }

    /// Builds the stretcher and the buffers, once. Allocates: the worker's thread (or an
    /// offline render) runs it, before the voice's first frames.
    pub(crate) fn prepare(&mut self) {
        if self.stretch.is_none() {
            self.install(&mut Some(self.spec().build()));
        }
    }

    /// Takes the stretcher and buffers built from [`StretchVoice::spec`] on another thread out of
    /// `parts`. Never allocates or frees: a voice that has its own already leaves `parts` where
    /// they are, for the caller to drop off the audio thread.
    pub(crate) fn install(&mut self, parts: &mut Option<StretchParts>) {
        if self.stretch.is_some() {
            return;
        }
        let Some(StretchParts { stretch, fifo, input, chunk }) = parts.take() else { return };
        // The voice's own buffers are still the empty ones `with_clock` made, which free nothing.
        self.fifo = fifo;
        self.input = input;
        self.chunk = chunk;
        self.stretch = Some(stretch);
        self.started = false;
    }

    /// Whether the stretcher and the buffers are built.
    pub(crate) fn is_prepared(&self) -> bool {
        self.stretch.is_some()
    }

    /// The voice, prepared ([`StretchVoice::prepare`]).
    #[cfg(test)]
    pub(crate) fn prepared(mut self) -> Self {
        self.prepare();
        self
    }

    /// Rewinds the voice to the clip's first frame, for a transport restart or a relaunch.
    /// Never allocates; nothing for a voice not prepared yet.
    pub(crate) fn restart(&mut self) {
        self.seek(0);
    }

    /// Moves the voice so that its next frame is `frames` project frames after the clip's start.
    /// Runs the stretcher's start-up on the calling thread. Never allocates: a voice not
    /// prepared yet ([`StretchVoice::prepare`]) stays where it is.
    pub(crate) fn seek(&mut self, frames: u64) {
        if self.stretch.is_none() {
            return;
        }
        self.started = true;
        let at = (frames as f64 * self.step).round() as u64;
        let Some(piece) = self.piece(self.piece_at(at)) else { return };
        let into_piece = (at as f64 - piece.out_start * self.file_rate).max(0.0);
        let stream = (piece.stream_start + into_piece * piece.rate).floor();
        let Some(length) = self.stretch.as_ref().map(|s| s.output_seek_length(piece.rate as f32)) else { return };
        let channels = self.channels;
        let mut input = std::mem::take(&mut self.input);
        let length = length.min(input.len() / channels);
        if let Some(buffer) = input.get_mut(..length * channels) {
            self.read_stream(stream, buffer);
            if let Some(stretch) = self.stretch.as_mut() {
                stretch.output_seek(buffer);
            }
        }
        self.input = input;
        self.fed = stream + length as f64;
        self.produced = at;
        self.fifo_len = 0;
        self.fifo_read = 0;
        self.fifo_position = 0.0;
    }

    /// Where in the input stream voice frame `at` plays.
    fn stream_at(&self, at: u64) -> f64 {
        let Some(piece) = self.piece(self.piece_at(at)) else { return 0.0 };
        piece.stream_start + (at as f64 - piece.out_start * self.file_rate).max(0.0) * piece.rate
    }

    /// The piece at `place`, moved to its repeat.
    fn piece(&self, place: Place) -> Option<Piece> {
        let mut piece = *self.pieces.get(place.index)?;
        if let Some(r) = self.repeat {
            piece.out_start += place.turn as f64 * r.out;
            piece.stream_start += place.turn as f64 * r.stream;
        }
        Some(piece)
    }

    /// The voice frame where the piece at `place` starts.
    fn out_frame(&self, place: Place) -> Option<u64> {
        self.piece(place).map(|p| (p.out_start * self.file_rate).round() as u64)
    }

    /// The place after `place`: the next piece, or the repeat's first one again.
    fn next(&self, place: Place) -> Option<Place> {
        if place.index + 1 < self.pieces.len() {
            return Some(Place { index: place.index + 1, turn: place.turn });
        }
        self.repeat.map(|r| Place { index: r.first, turn: place.turn + 1 })
    }

    /// The place playing at voice frame `at` (the last piece past the end of a playlist that does
    /// not repeat).
    fn piece_at(&self, at: u64) -> Place {
        let file_rate = self.file_rate;
        let first = |turn: u64| Place { index: self.repeat.map_or(0, |r| r.first), turn };
        let mut turn = 0;
        if let (Some(r), Some(start)) = (self.repeat, self.out_frame(first(0)))
            && at >= start
        {
            let estimate = ((at - start) as f64 / file_rate / r.out).floor();
            turn = if estimate.is_finite() && estimate > 0.0 { estimate as u64 } else { 0 };
            // The estimate can be off by one at a turn's first frame.
            while turn > 0 && self.out_frame(first(turn)).is_some_and(|f| at < f) {
                turn -= 1;
            }
            while self.out_frame(first(turn + 1)).is_some_and(|f| at >= f) {
                turn += 1;
            }
        }
        // The pieces are sorted by `out_start`: a binary search, as this runs per chunk.
        let from = if turn > 0 { first(turn).index } else { 0 };
        let shift = self.repeat.map_or(0.0, |r| turn as f64 * r.out);
        let rest = self.pieces.get(from..).unwrap_or_default();
        let count = rest.partition_point(|p| ((p.out_start + shift) * file_rate).round() as u64 <= at);
        Place { index: (from + count).saturating_sub(1), turn }
    }

    /// The place whose input-stream frames include `position`.
    fn piece_in_stream(&self, position: f64) -> Place {
        let mut turn = 0;
        let mut position = position;
        if let Some(r) = self.repeat
            && let Some(first) = self.pieces.get(r.first)
            && position >= first.stream_start + r.stream
        {
            let turns = ((position - first.stream_start) / r.stream).floor();
            turn = if turns.is_finite() && turns > 0.0 { turns as u64 } else { 0 };
            position -= turn as f64 * r.stream;
        }
        let from = if turn > 0 { self.repeat.map_or(0, |r| r.first) } else { 0 };
        let rest = self.pieces.get(from..).unwrap_or_default();
        Place { index: (from + rest.partition_point(|p| p.stream_start <= position)).saturating_sub(1), turn }
    }

    /// Fills `out` (interleaved) with the input stream from frame `from` on. Past the playlist
    /// the file carries on; past the file there is silence.
    fn read_stream(&self, from: f64, out: &mut [f32]) {
        let channels = self.channels;
        let file_frames = self.data.frames();
        let mut place = self.piece_in_stream(from);
        let mut piece = self.piece(place);
        let mut next = self.next(place).and_then(|n| Some((n, self.piece(n)?)));
        for (i, frame) in out.chunks_exact_mut(channels).enumerate() {
            let position = from + i as f64;
            while let Some((n, p)) = next
                && p.stream_start <= position
            {
                place = n;
                piece = Some(p);
                next = self.next(place).and_then(|n| Some((n, self.piece(n)?)));
            }
            let file_frame = piece.map_or(-1.0, |p| p.src_start + (position - p.stream_start));
            let file_frame = file_frame.floor();
            for (c, s) in frame.iter_mut().enumerate() {
                *s = if file_frame >= 0.0 && (file_frame as usize) < file_frames {
                    self.data.samples.get(file_frame as usize * channels + c).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
            }
        }
    }

    /// Runs the stretcher for up to one chunk, at one speed, and appends the output to the FIFO.
    fn run_chunk(&mut self) {
        let channels = self.channels;
        // Make room by dropping frames that have been read.
        if self.fifo_read > 0 {
            let read = self.fifo_read.min(self.fifo_len);
            if let Some(unread) = self.fifo.get(read * channels..self.fifo_len * channels) {
                let unread = unread.len();
                self.fifo.copy_within(read * channels..read * channels + unread, 0);
            }
            self.fifo_len -= read;
            self.fifo_read = 0;
        }
        let free = self.fifo.len() / channels - self.fifo_len;
        let mut frames = CHUNK_FRAMES.min(free) as u64;
        if frames == 0 {
            return;
        }
        // What is fed now is heard `output_latency` frames after these frames are made, so the
        // input follows the piece heard then: a speed change reaches the input that much before
        // it is heard, and the input never runs ahead or behind the timing. One speed per call:
        // stop where that piece ends.
        let Some((latency, input_latency)) =
            self.stretch.as_ref().map(|s| (s.output_latency() as u64, s.input_latency()))
        else {
            return;
        };
        let heard = self.produced + latency;
        let place = self.piece_at(heard);
        if let Some(boundary) = self.next(place).and_then(|n| self.out_frame(n))
            && boundary > heard
        {
            frames = frames.min(boundary - heard);
        }
        let rate = self.piece(place).map_or(1.0, |p| p.rate);
        // Never less than half the speed's input: a call without input makes the stretcher freeze
        // its sound (after a seek, or a playlist that jumps). At half the input the stretch stays
        // clean at 1x and above, and any lead left over drains within a few calls.
        let want = self.stream_at(heard + frames) + input_latency as f64;
        let fed = want.max(self.fed + frames as f64 * rate * 0.5);
        let input_frames = ((fed.floor() - self.fed.floor()).max(0.0) as usize).min(self.input.len() / channels);
        let frames = frames as usize;
        let mut input = std::mem::take(&mut self.input);
        let mut chunk = std::mem::take(&mut self.chunk);
        if let (Some(input_buffer), Some(output)) =
            (input.get_mut(..input_frames * channels), chunk.get_mut(..frames * channels))
        {
            self.read_stream(self.fed.floor(), input_buffer);
            if let Some(stretch) = self.stretch.as_mut() {
                stretch.process(input_buffer, output);
            }
        }
        self.input = input;
        self.chunk = chunk;
        self.fed = fed;
        let at = self.fifo_len * channels;
        if let (Some(dst), Some(src)) =
            (self.fifo.get_mut(at..at + frames * channels), self.chunk.get(..frames * channels))
        {
            dst.copy_from_slice(src);
            self.fifo_len += frames;
            self.produced += frames as u64;
        }
    }

    /// Renders the next `out.len() / 2` project frames as interleaved stereo into `out`: silence
    /// while the voice is not prepared. Never allocates.
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        if self.stretch.is_none() {
            out.fill(0.0);
            return;
        }
        // Not played or moved yet: from the clip's start.
        if !self.started {
            self.seek(0);
        }
        let channels = self.channels;
        for frame in out.as_chunks_mut::<2>().0 {
            // Linear interpolation needs this frame and the next.
            while self.fifo_len < self.fifo_read + 2 {
                // Progress is counted in frames produced: compacting the FIFO can leave its length
                // unchanged even when a chunk was added.
                let before = self.produced;
                self.run_chunk();
                if self.produced == before {
                    break;
                }
            }
            let at = |i: usize, c: usize| self.fifo.get(i * channels + c.min(channels - 1)).copied().unwrap_or(0.0);
            let t = self.fifo_position as f32;
            let i = self.fifo_read;
            frame[0] = at(i, 0) + (at(i + 1, 0) - at(i, 0)) * t;
            frame[1] = at(i, 1) + (at(i + 1, 1) - at(i, 1)) * t;
            self.fifo_position += self.step;
            let whole = self.fifo_position.floor();
            self.fifo_read += whole as usize;
            self.fifo_position -= whole;
        }
    }
}

#[cfg(test)]
#[path = "stretch_tests.rs"]
mod tests;
