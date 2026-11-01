// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The recording thread (`docs/spec/recording.md` › Audio files): it empties the engine's
//! recording rings as they fill, whatever the window or an agent is doing, hands the notes to
//! the session and writes each audio take into its WAV file. The render thread never writes a
//! file; the session never waits for one.
//!
//! An audio take starts at the first frame whose beat reaches the take's start and ends a number
//! of beats later; frames lost because the ring was full are written as silence, so later sound
//! keeps its beat. A file that cannot be written ends its take, keeping what was written.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use suhina_engine::{RecordReceiver, RecordedAudio, RecordedJump, RecordedNote};
use suhina_model::TrackId;

/// How long the thread rests between looks at the rings while it writes a take: the audio ring
/// holds seconds.
const LOOK_EVERY: Duration = Duration::from_millis(5);
/// How long it rests with no take to write: notes still come through, a little later.
const LOOK_IDLE: Duration = Duration::from_millis(40);
/// How long an ended take waits for frames that have not come before its file is finished
/// anyway (the transport stopped, the device went away).
const END_WAIT: Duration = Duration::from_millis(500);
/// The longest gap of lost frames filled with silence, so a hostile position cannot fill a disk.
const MAX_GAP_SECONDS: u64 = 10;

/// What the session asks of the thread.
pub(crate) enum Order {
    /// Record `track`'s input into `path` from the first frame whose beat (the launcher clock's
    /// with `by_clock`, else the arrangement's) reaches `start`.
    Start { track: TrackId, path: PathBuf, start: f64, by_clock: bool },
    /// End `track`'s take `beats` beats after its start (now, when it has not started).
    End { track: TrackId, beats: f64 },
    /// Answer on `ack` once the thread has emptied the rings and sent every report for what the
    /// engine had recorded when this order was sent. With `flush`, no more audio is coming: an
    /// ended take is finished at once rather than after [`END_WAIT`].
    Sync { ack: Sender<()>, flush: bool },
}

/// What the thread tells the session.
#[derive(Debug)]
pub(crate) enum Report {
    Note(RecordedNote),
    /// The arrangement's playhead jumped: a new pass over the beats begins (#909).
    Jump(RecordedJump),
    /// `track`'s take wrote its first frame, which lines up with renderer frame `position`.
    Began {
        track: TrackId,
        path: PathBuf,
        position: u64,
    },
    /// A take's file is finished: its frames and channels, and why it ended early, if it did.
    Finished {
        track: TrackId,
        path: PathBuf,
        frames: u64,
        channels: u16,
        error: Option<String>,
    },
    /// Frames of `track`'s take written so far, and the peaks of the frames written since the
    /// last report, one per [`PEAK_FRAMES`], for the take's waveform while it records (#909).
    Written {
        track: TrackId,
        frames: u64,
        peaks: Vec<f32>,
    },
}

/// Frames per peak of a recording take's waveform.
pub(crate) const PEAK_FRAMES: u64 = 512;

/// The session's end of the thread.
pub(crate) struct RecordThread {
    orders: Sender<Order>,
    reports: Receiver<Report>,
}

impl RecordThread {
    /// Starts the thread on `receiver`, whose audio is at `sample_rate`. It ends when the session
    /// lets go of its end, or the engine of its own and nothing is left to write.
    pub(crate) fn start(receiver: RecordReceiver, sample_rate: u32) -> Result<Self, String> {
        let (orders, order_end) = channel();
        let (report_end, reports) = channel();
        std::thread::Builder::new()
            .name("suhina-record".into())
            .spawn(move || run(receiver, sample_rate, &order_end, &report_end))
            .map_err(|e| format!("could not start the recording thread: {e}"))?;
        Ok(Self { orders, reports })
    }

    pub(crate) fn send(&self, order: Order) -> Result<(), String> {
        self.orders.send(order).map_err(|_| "the recording thread has stopped".to_string())
    }

    /// Waits until the thread has taken in everything the engine recorded so far and reported it,
    /// so the next [`RecordThread::report`] calls see it all: the synchronization offline renders
    /// and tests use instead of waiting for the thread's own pace. `flush` as in [`Order::Sync`].
    pub(crate) fn sync(&self, flush: bool) {
        let (ack, answered) = channel();
        if self.send(Order::Sync { ack, flush }).is_ok() {
            let _ = answered.recv();
        }
    }

    /// The next report, if one is waiting.
    pub(crate) fn report(&self) -> Option<Report> {
        self.reports.try_recv().ok()
    }
}

/// One audio take being written.
struct Take {
    track: TrackId,
    path: PathBuf,
    start: f64,
    by_clock: bool,
    /// The file, once its first frame came, the position of that frame, the channels, and the
    /// beats per frame then.
    writer: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
    first: Option<u64>,
    channels: u16,
    beats_per_frame: f64,
    /// Frames written; the take ends `end` beats after its first frame.
    written: u64,
    end: Option<f64>,
    /// When it was told to end or last had a chunk, whichever is later, and why it failed.
    ended: Option<Instant>,
    error: Option<String>,
    /// The loudest sample of the peak being gathered, and the peaks finished since the last
    /// report.
    peak: f32,
    peaks: Vec<f32>,
    /// The last time the arrangement's playhead jumped while this take was written (a loop
    /// going back, a click on the ruler): its end is counted from there, so every pass before
    /// it is kept whole (#909).
    last_jump: Option<RecordedJump>,
}

impl Take {
    /// Counts one more frame written, with `level`, finishing a peak every [`PEAK_FRAMES`].
    fn add_to_peak(&mut self, level: f32) {
        self.peak = self.peak.max(level);
        self.written += 1;
        if self.written.is_multiple_of(PEAK_FRAMES) {
            // At most a report's worth waits; the session gathers and caps them.
            if self.peaks.len() < 1 << 16 {
                self.peaks.push(self.peak.min(1.0));
            }
            self.peak = 0.0;
        }
    }

    /// Frames from its first that the take ends at, once known.
    fn until(&self) -> Option<u64> {
        let end = self.end?;
        if end.is_nan() || end <= 0.0 {
            return Some(0);
        }
        // After a jump the end beat is on the pass that jump began: the frames up to the jump,
        // then the beats from where it landed. Without one, the beats from the start.
        if let (Some(jump), Some(first)) = (self.last_jump, self.first) {
            let step = jump.beats_per_frame;
            let after = (self.start + end - jump.beat).max(0.0) / step;
            return (step > 0.0 && after.is_finite())
                .then(|| jump.position.saturating_sub(first).saturating_add(after.round() as u64));
        }
        let frames = end / self.beats_per_frame;
        (self.beats_per_frame > 0.0 && frames.is_finite()).then(|| frames.round() as u64)
    }
}

fn run(mut receiver: RecordReceiver, sample_rate: u32, orders: &Receiver<Order>, reports: &Sender<Report>) {
    let mut takes: Vec<Take> = Vec::new();
    let mut last_report = Instant::now();
    // An order that woke the thread from its rest, handled at the top of the next look.
    let mut woken = None;
    loop {
        let mut syncs: Vec<Sender<()>> = Vec::new();
        let mut flush = false;
        loop {
            match woken.take().map_or_else(|| orders.try_recv(), Ok) {
                Ok(Order::Start { track, path, start, by_clock }) => {
                    takes.retain(|t| t.track != track);
                    takes.push(Take {
                        track,
                        path,
                        start,
                        by_clock,
                        writer: None,
                        first: None,
                        channels: 1,
                        beats_per_frame: 0.0,
                        written: 0,
                        end: None,
                        ended: None,
                        error: None,
                        peak: 0.0,
                        peaks: Vec::new(),
                        last_jump: None,
                    });
                }
                Ok(Order::End { track, beats }) => {
                    if let Some(take) = takes.iter_mut().find(|t| t.track == track && t.ended.is_none()) {
                        take.ended = Some(Instant::now());
                        take.end = Some(if beats.is_finite() { beats } else { 0.0 });
                    }
                }
                Ok(Order::Sync { ack, flush: f }) => {
                    syncs.push(ack);
                    flush |= f;
                }
                Err(TryRecvError::Empty) => break,
                // The session is gone: what is open is finished and nobody hears of it.
                Err(TryRecvError::Disconnected) => {
                    for take in &mut takes {
                        finish(take);
                    }
                    return;
                }
            }
        }
        if receiver.is_abandoned() && takes.is_empty() {
            return;
        }
        while let Some(note) = receiver.next_note() {
            let _ = reports.send(Report::Note(note));
        }
        while let Some(jump) = receiver.next_jump() {
            // An arrangement take being written counts its end from the last jump.
            for take in takes.iter_mut().filter(|t| !t.by_clock && t.first.is_some_and(|f| jump.position >= f)) {
                take.last_jump = Some(jump);
            }
            let _ = reports.send(Report::Jump(jump));
        }
        while let Some(chunk) = receiver.next_audio() {
            if let Some(take) = takes.iter_mut().find(|t| t.track == chunk.track && t.error.is_none()) {
                let began = take.first.is_some();
                write(take, &chunk, sample_rate);
                if let (false, Some(position)) = (began, take.first) {
                    let _ = reports.send(Report::Began { track: take.track, path: take.path.clone(), position });
                }
                if let Some(ended) = take.ended.as_mut() {
                    *ended = Instant::now();
                }
            }
        }
        // Takes that are done: written to their end, failed, or waited for long enough.
        let mut index = 0;
        while let Some(take) = takes.get_mut(index) {
            let reached = take.until().is_some_and(|until| take.written >= until);
            // Ended, and no chunk came for a while: the transport stopped or the device went.
            let waited = take.ended.is_some_and(|at| flush || at.elapsed() >= END_WAIT);
            let gone = receiver.is_abandoned() && take.ended.is_some();
            if reached || take.error.is_some() || waited || gone {
                finish(take);
                let take = takes.remove(index);
                // Frames written past the end before it was known are in the file, not the take.
                let frames = take.until().map_or(take.written, |until| take.written.min(until));
                let _ = reports.send(Report::Finished {
                    track: take.track,
                    path: take.path,
                    frames,
                    channels: take.channels,
                    error: take.error,
                });
            } else {
                index += 1;
            }
        }
        if last_report.elapsed() >= Duration::from_millis(100) {
            last_report = Instant::now();
            for take in &mut takes {
                let peaks = std::mem::take(&mut take.peaks);
                let _ = reports.send(Report::Written { track: take.track, frames: take.written, peaks });
            }
        }
        for ack in syncs {
            let _ = ack.send(());
        }
        // Rest, but an order ends the rest at once.
        let rest = if takes.is_empty() { LOOK_IDLE } else { LOOK_EVERY };
        woken = orders.recv_timeout(rest).ok();
    }
}

/// Writes what of `chunk` belongs to `take`.
fn write(take: &mut Take, chunk: &RecordedAudio, sample_rate: u32) {
    let channels = usize::from(chunk.channels.clamp(1, 2));
    let frames = usize::from(chunk.frames);
    let first = match take.first {
        Some(first) => first,
        None => {
            // The take starts at the first frame whose beat reaches its start.
            let beat = if take.by_clock { chunk.clock } else { chunk.beat };
            let step = chunk.beats_per_frame;
            if !(step > 0.0 && step.is_finite() && beat.is_finite()) {
                return;
            }
            let into = ((take.start - beat) / step).ceil().max(0.0);
            if into >= frames as f64 {
                return;
            }
            let first = chunk.position + into as u64;
            let spec = hound::WavSpec {
                channels: channels as u16,
                sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let made = take
                .path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|e| e.to_string())
                .and_then(|()| hound::WavWriter::create(&take.path, spec).map_err(|e| e.to_string()));
            match made {
                Ok(writer) => take.writer = Some(writer),
                Err(e) => {
                    take.error = Some(format!("could not write {}: {e}", take.path.display()));
                    return;
                }
            }
            take.first = Some(first);
            take.channels = channels as u16;
            take.beats_per_frame = step;
            first
        }
    };
    let until = take.until().unwrap_or(u64::MAX);
    let next = first + take.written;
    // Out of the take while it is written to, so the take's counts can change meanwhile.
    let Some(mut writer) = take.writer.take() else { return };
    // Frames lost before this chunk are silence, so later sound keeps its beat.
    let gap = chunk.position.saturating_sub(next).min(MAX_GAP_SECONDS * u64::from(sample_rate));
    let mut failed = None;
    for _ in 0..gap {
        if take.written >= until || failed.is_some() {
            break;
        }
        for _ in 0..take.channels {
            if let Err(e) = writer.write_sample(0.0f32) {
                failed = Some(e);
            }
        }
        take.add_to_peak(0.0);
    }
    let skip = (first + take.written).saturating_sub(chunk.position);
    let samples = chunk.samples();
    for frame in (skip as usize)..frames {
        if take.written >= until || failed.is_some() {
            break;
        }
        for channel in 0..usize::from(take.channels) {
            // A chunk whose channels changed (another input) keeps the file's: mono from left.
            let sample = samples.get(frame * channels + channel.min(channels - 1)).copied().unwrap_or(0.0);
            if let Err(e) = writer.write_sample(sample) {
                failed = Some(e);
                break;
            }
            take.peak = take.peak.max(if sample.is_finite() { sample.abs() } else { 0.0 });
        }
        take.add_to_peak(0.0);
    }
    take.writer = Some(writer);
    if let Some(e) = failed {
        take.error = Some(format!("could not write {}: {e}; the take keeps what was written", take.path.display()));
    }
}

/// Finishes the file; an error doing so is the take's error.
fn finish(take: &mut Take) {
    if let Some(writer) = take.writer.take()
        && let Err(e) = writer.finalize()
        && take.error.is_none()
    {
        take.error = Some(format!("could not finish {}: {e}", take.path.display()));
    }
}
