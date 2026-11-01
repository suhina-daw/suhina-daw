// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recording ([`docs/spec/recording.md`](../../../docs/spec/recording.md)): while it is on, the
//! render thread hands out what the armed tracks play, each piece stamped with the beat it lines
//! up with, through two lock-free rings made before the renderer moves to the audio thread
//! ([`crate::Renderer::controller`] makes them; the session takes the reading end with
//! [`crate::Controller::take_recorder`]):
//!
//! - **notes**: every key a MIDI input plays on an armed track, down and up, at the beat that was
//!   heard when it was played: its frame less the key-to-sound latency (the live notes' fixed
//!   delay and the output's latency);
//! - **audio**: every armed track's input channel or pair, in chunks of [`RECORD_CHUNK`] frames,
//!   at the beat that was heard when it reached the input: its frame less the output's latency,
//!   the input device's and what waited in the input ring.
//!
//! - **jumps**: where the arrangement's playhead jumped while recording (the loop wrapping, a
//!   click on the ruler), so the session can keep each pass over the same beats as a take of its
//!   own (take lanes, #909).
//!
//! The session decides what a take keeps; the render thread only copies. Nothing here allocates,
//! locks, blocks or does I/O on the render thread: what finds a ring full is dropped and counted.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};
use suhina_model::TrackId;

/// Notes the ring holds until the session takes them: far more than are played between two
/// looks.
pub const RECORD_NOTES: usize = 4096;
/// Frames in one chunk of recorded audio.
pub const RECORD_CHUNK: usize = 64;
/// Chunks the audio ring holds: about eleven seconds of one track at 48 kHz, so a writer thread
/// that is late by a few seconds loses nothing.
pub const RECORD_CHUNKS: usize = 8192;
/// Jumps the ring holds until the session takes them.
pub const RECORD_JUMPS: usize = 256;

/// A key played on an armed track while recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedNote {
    pub track: TrackId,
    /// The launcher clock's beat that was heard when the key went down or the sound was let go
    /// (what launch boundaries count in).
    pub clock: f64,
    /// The arrangement's beat then.
    pub beat: f64,
    pub key: u8,
    /// From 0 to 1 for a key going down; `None` for the sound let go.
    pub velocity: Option<f32>,
    /// The renderer frame that was heard then: frames only go forward, so the pass a note
    /// belongs to is found by it, however the playhead jumped.
    pub position: u64,
}

/// The arrangement's playhead jumped while recording: at renderer frame `position` it was at
/// `beat`, where it would have been at `from` had it played on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedJump {
    pub position: u64,
    pub from: f64,
    pub beat: f64,
    /// Beats per frame at the tempo playing.
    pub beats_per_frame: f64,
}

/// Up to [`RECORD_CHUNK`] frames of an armed track's input while recording.
#[derive(Debug, Clone, Copy)]
pub struct RecordedAudio {
    pub track: TrackId,
    /// The renderer frame the first frame lines up with: the frame that was heard when it reached
    /// the input. Frames follow each other without gaps unless the ring was full.
    pub position: u64,
    /// The launcher clock's beat and the arrangement's beat at `position`.
    pub clock: f64,
    pub beat: f64,
    /// Beats per frame at the tempo playing.
    pub beats_per_frame: f64,
    /// 1 for one input channel, 2 for a pair.
    pub channels: u8,
    /// Frames in `samples`, at most [`RECORD_CHUNK`].
    pub frames: u16,
    /// `channels` samples per frame, interleaved.
    pub samples: [f32; RECORD_CHUNK * 2],
}

impl RecordedAudio {
    /// The samples of the chunk's frames.
    pub fn samples(&self) -> &[f32] {
        let len = usize::from(self.frames) * usize::from(self.channels.clamp(1, 2));
        self.samples.get(..len).unwrap_or_default()
    }
}

/// How far a block's beat may be from where the last block's led before it counts as a jump:
/// more than rounding, less than any jump a user makes.
const JUMP_BEATS: f64 = 1e-3;

/// The session's end of the recording rings.
pub struct RecordReceiver {
    notes: Consumer<RecordedNote>,
    audio: Consumer<RecordedAudio>,
    jumps: Consumer<RecordedJump>,
    lost_notes: Arc<AtomicU64>,
    lost_frames: Arc<AtomicU64>,
}

impl RecordReceiver {
    /// The next note recorded, in the order they were played.
    pub fn next_note(&mut self) -> Option<RecordedNote> {
        self.notes.pop().ok()
    }

    /// The next jump of the playhead, in order.
    pub fn next_jump(&mut self) -> Option<RecordedJump> {
        self.jumps.pop().ok()
    }

    /// The next chunk of recorded audio, in order.
    pub fn next_audio(&mut self) -> Option<RecordedAudio> {
        self.audio.pop().ok()
    }

    /// Notes and frames of audio dropped so far because the session did not take them in time.
    pub fn lost(&self) -> (u64, u64) {
        (self.lost_notes.load(Ordering::Relaxed), self.lost_frames.load(Ordering::Relaxed))
    }

    /// The renderer is gone (its output closed): nothing more will come.
    pub fn is_abandoned(&self) -> bool {
        self.notes.is_abandoned()
    }
}

impl std::fmt::Debug for RecordReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordReceiver").field("lost", &self.lost()).finish()
    }
}

/// The render thread's end: whether it records, the rings, and where this block is.
pub(crate) struct Recorder {
    on: bool,
    notes: Option<Producer<RecordedNote>>,
    audio: Option<Producer<RecordedAudio>>,
    jumps: Option<Producer<RecordedJump>>,
    lost_notes: Arc<AtomicU64>,
    lost_frames: Arc<AtomicU64>,
    /// The output's latency and the input device's, in frames, as the output last said.
    output_latency: u64,
    input_latency: u64,
    /// This block: whether the transport runs, its first renderer frame, the beats there and
    /// beats per frame.
    running: bool,
    position: u64,
    clock: f64,
    beat: f64,
    beats_per_frame: f64,
    /// Frames between a note played and the beat heard then, and between the input read and
    /// the beat heard when it was captured.
    note_latency: u64,
    audio_latency: u64,
    /// Frames still to record after the transport stopped or recording went off: what was
    /// played before is still on its way through the output and the input (`audio_latency`),
    /// and a take ends where it was heard, so its last frames come in after the stop.
    tail: u64,
}

impl Recorder {
    pub(crate) fn new() -> Self {
        Self {
            on: false,
            notes: None,
            audio: None,
            jumps: None,
            lost_notes: Arc::new(AtomicU64::new(0)),
            lost_frames: Arc::new(AtomicU64::new(0)),
            output_latency: 0,
            input_latency: 0,
            running: false,
            position: 0,
            clock: 0.0,
            beat: 0.0,
            beats_per_frame: 0.0,
            note_latency: 0,
            audio_latency: 0,
            tail: 0,
        }
    }

    /// New rings; the previous receiver gets nothing more. Allocates: call it off the audio
    /// thread.
    pub(crate) fn receiver(&mut self) -> RecordReceiver {
        let (notes, note_reader) = RingBuffer::new(RECORD_NOTES);
        let (audio, audio_reader) = RingBuffer::new(RECORD_CHUNKS);
        let (jumps, jump_reader) = RingBuffer::new(RECORD_JUMPS);
        self.notes = Some(notes);
        self.audio = Some(audio);
        self.jumps = Some(jumps);
        RecordReceiver {
            notes: note_reader,
            audio: audio_reader,
            jumps: jump_reader,
            lost_notes: Arc::clone(&self.lost_notes),
            lost_frames: Arc::clone(&self.lost_frames),
        }
    }

    pub(crate) fn set(&mut self, on: bool) {
        if self.on && !on && self.running {
            self.tail = self.audio_latency;
        }
        self.on = on;
    }

    pub(crate) fn set_latency(&mut self, output: u32, input: u32) {
        self.output_latency = u64::from(output);
        self.input_latency = u64::from(input);
    }

    /// Whether this block's input is recorded: while recording, and for the latency after it
    /// stops, so a take gets its last frames.
    pub(crate) fn takes_audio(&self) -> bool {
        (self.on && self.running || self.tail > 0) && self.audio.is_some()
    }

    /// Where a block starting at renderer frame `position` is: the launcher clock's beat and the
    /// arrangement's there, and beats per frame. `live_delay` is the frames a stamped note
    /// plays after it was played; `input_waiting` the frames that waited in the input ring
    /// before this block's were read.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        &mut self,
        running: bool,
        position: u64,
        clock: f64,
        beat: f64,
        beats_per_frame: f64,
        live_delay: u64,
        input_waiting: u64,
    ) {
        // A block that does not start where the last one's beats led: the playhead jumped.
        if self.on && self.running && running && position > self.position {
            let from = self.beat + (position - self.position) as f64 * self.beats_per_frame;
            if (beat - from).abs() > JUMP_BEATS
                && let Some(ring) = self.jumps.as_mut()
            {
                let jump = RecordedJump { position, from, beat, beats_per_frame };
                let _full = ring.push(jump);
            }
        }
        let elapsed = position.saturating_sub(self.position);
        if self.on && self.running && !running {
            // The transport stopped: the last frames played are still to come in.
            self.tail = self.audio_latency;
        } else if self.on && running {
            self.tail = 0;
        } else {
            self.tail = self.tail.saturating_sub(elapsed);
        }
        let tailing = self.tail > 0 && !(self.on && running);
        self.running = running;
        if tailing {
            // The beats go on from where recording stopped, for the frames that come in after.
            let step = elapsed as f64 * self.beats_per_frame;
            self.clock += step;
            self.beat += step;
        } else {
            self.clock = clock;
            self.beat = beat;
            self.beats_per_frame = if beats_per_frame.is_finite() { beats_per_frame.max(0.0) } else { 0.0 };
            self.note_latency = live_delay.saturating_add(self.output_latency);
            self.audio_latency = self.output_latency.saturating_add(self.input_latency).saturating_add(input_waiting);
        }
        self.position = position;
    }

    /// The beats at renderer frame `position` less `latency` frames, counted from this block.
    fn beats_at(&self, position: u64, latency: u64) -> (f64, f64) {
        let frames = position as f64 - self.position as f64 - latency as f64;
        let step = frames * self.beats_per_frame;
        (self.clock + step, self.beat + step)
    }

    /// Records a key of `track` played at renderer frame `position`: down with `velocity`, or let
    /// go with `None`. Nothing while recording is off or the transport is stopped.
    pub(crate) fn note(&mut self, track: TrackId, position: u64, key: u8, velocity: Option<f32>) {
        if !(self.on && self.running) {
            return;
        }
        let (clock, beat) = self.beats_at(position, self.note_latency);
        let heard = position.saturating_sub(self.note_latency);
        let Some(ring) = self.notes.as_mut() else { return };
        let velocity = velocity.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 });
        if ring.push(RecordedNote { track, clock, beat, key, velocity, position: heard }).is_err() {
            self.lost_notes.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records `frames` frames of `track`'s input from frame `from` of this block: `channels`
    /// (1 or 2) samples per frame from `sample(frame, channel)`.
    pub(crate) fn audio(
        &mut self,
        track: TrackId,
        channels: u8,
        from: usize,
        frames: usize,
        sample: impl Fn(usize, usize) -> f32,
    ) {
        if !self.takes_audio() {
            return;
        }
        // After a stop, only the frames still to come in.
        let frames =
            if self.on && self.running { frames } else { frames.min(usize::try_from(self.tail).unwrap_or(usize::MAX)) };
        let channels = channels.clamp(1, 2);
        let mut done = 0;
        while done < frames {
            let count = (frames - done).min(RECORD_CHUNK);
            let first = from + done;
            let position = (self.position + first as u64).saturating_sub(self.audio_latency);
            let (clock, beat) = self.beats_at(self.position + first as u64, self.audio_latency);
            let mut chunk = RecordedAudio {
                track,
                position,
                clock,
                beat,
                beats_per_frame: self.beats_per_frame,
                channels,
                frames: count as u16,
                samples: [0.0; RECORD_CHUNK * 2],
            };
            for frame in 0..count {
                for channel in 0..usize::from(channels) {
                    if let Some(slot) = chunk.samples.get_mut(frame * usize::from(channels) + channel) {
                        *slot = sample(first + frame, channel);
                    }
                }
            }
            let pushed = self.audio.as_mut().is_some_and(|ring| ring.push(chunk).is_ok());
            if !pushed {
                self.lost_frames.fetch_add(count as u64, Ordering::Relaxed);
            }
            done += count;
        }
    }
}
