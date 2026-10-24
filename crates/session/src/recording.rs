// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recording ([`docs/spec/recording.md`](../../../docs/spec/recording.md)): takes of what the
//! armed tracks play, into a clip slot or the arrangement, with an optional count-in. The engine
//! hands out what it records (notes, and the input in chunks) through its recording rings; the
//! recording thread ([`crate::record_writer`]) empties them, writing audio takes to files, and
//! the session turns each finished take into a clip, one undo step per take.
//!
//! A take's start and end are beats: the launcher clock's for a slot (launch boundaries count in
//! it), the arrangement's for the arrangement. A take that has an end is finished once the clock
//! is past it by a moment (notes played before the end reach the engine a little after it) or,
//! for audio, once its file is.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use suhina_engine::{LaunchCommand, RecordedNote};
use suhina_model::{AudioRegion, InputChannels, LaneData, LoopRange, Note, Project, StretchMode, TrackId};

use crate::files::TrackKind;

use crate::commands::{column_index, scene_index, track_id};
use crate::record_writer::{Order, RecordThread, Report};
use crate::{CommandError, Session};

/// The count-ins that can be chosen, in bars.
pub const COUNT_IN_BARS: [u8; 4] = [0, 1, 2, 4];
/// The folder next to the project file that audio takes are written to.
pub const RECORDED_FOLDER: &str = "Recorded";
/// How long past its end a take waits for notes still on their way: played before the end,
/// they reach the engine one key-to-sound latency after it.
const SETTLE: Duration = Duration::from_millis(300);
/// How long a slot whose take became a clip is asked to play until the engine has the clip.
const RESUME_FOR: Duration = Duration::from_secs(2);
/// The shortest recorded note, in beats.
const MIN_NOTE: f64 = 1.0 / 128.0;

/// Where a take goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Sampler { device: usize },
    Slot { scene: usize },
    Arrangement,
}

/// One track's take.
struct Take {
    track: TrackId,
    target: Target,
    /// The file an audio take is written to; `None` for a MIDI take.
    file: Option<PathBuf>,
    /// The beat it starts at: the launcher clock's for a slot, the arrangement's otherwise.
    start: f64,
    /// The beat it ends at, once asked, and when that was asked.
    end: Option<f64>,
    asked: Option<Instant>,
    /// Whether the slot plays the clip once it is made.
    play: bool,
    notes: Vec<RecordedNote>,
    /// An audio take: frames written so far, whether it was told its end, and its file once
    /// finished (frames, channels, why it ended early).
    written: u64,
    end_sent: bool,
    finished: Option<(u64, u16, Option<String>)>,
}

/// The session's recording state; arming stays with the session.
#[derive(Default)]
pub(crate) struct Recording {
    /// The recording threads: the newest reads the playing engine; older ones finish the takes
    /// of an engine that was replaced.
    threads: Vec<RecordThread>,
    sample_rate: u32,
    count_in_bars: u8,
    /// The arrangement records (`transport.record`).
    arrangement: bool,
    takes: Vec<Take>,
    /// The engine was told to record and not yet told to stop.
    engine_on: bool,
    notices: Vec<String>,
    /// Slots whose new clip should play: column, slot, the clock beat it started at, until when.
    resumes: Vec<(usize, usize, f64, Instant)>,
}

impl Recording {
    /// Reads what a new engine records from now on.
    pub(crate) fn attach(&mut self, receiver: Option<suhina_engine::RecordReceiver>, sample_rate: u32) {
        self.engine_on = false;
        self.sample_rate = sample_rate;
        let Some(receiver) = receiver else { return };
        match RecordThread::start(receiver, sample_rate) {
            Ok(thread) => self.threads.push(thread),
            Err(e) => self.notices.push(e),
        }
        // Threads of engines gone with nothing left to write are let go.
        let keep = self.threads.len().saturating_sub(4);
        if keep > 0 && self.takes.iter().all(|t| t.file.is_none()) {
            self.threads.drain(..keep);
        }
    }

    fn take_on(&self, track: TrackId) -> Option<&Take> {
        self.takes.iter().find(|t| t.track == track)
    }
}

/// What an armed track records: audio when it has an audio input, notes when it plays notes.
fn kind_of(p: &Project, track: TrackId) -> Option<bool> {
    let t = p.track(track)?;
    if t.audio_input.channels != InputChannels::None {
        Some(true)
    } else if t.content.notes {
        Some(false)
    } else {
        None
    }
}

fn track_name(p: &Project, track: TrackId) -> String {
    p.track(track).map(|t| t.name.clone()).unwrap_or_default()
}

impl Session {
    pub fn recording_sampler(&self, track: TrackId, device: usize) -> bool {
        self.live
            .recording
            .takes
            .iter()
            .any(|t| t.track == track && t.target == Target::Sampler { device } && t.end.is_none())
    }
    /// The count-in recording waits for when it starts the transport, in bars (0 for none).
    /// The metronome as `transport.setMetronome` left it: on, its level in dB and its sound.
    pub fn metronome(&self) -> (bool, f64, suhina_engine::ClickSound) {
        (self.live.metronome.0, self.live.metronome.1, self.live.metronome.2)
    }

    pub fn count_in_bars(&self) -> u8 {
        self.live.recording.count_in_bars
    }

    /// Whether a take is being recorded now, into the arrangement or a slot: what auto-arm
    /// (#834) leaves the arm alone for, so a take is not cut off. A take asked to end counts
    /// until it is a clip: a slot's records on to its launch boundary, and either waits a
    /// moment after for the notes on their way.
    pub fn is_recording(&self) -> bool {
        !self.live.recording.takes.is_empty()
    }

    /// Whether the arrangement records (`transport.record`).
    pub fn records_arrangement(&self) -> bool {
        self.live.recording.arrangement
    }

    /// The slot `track` records into (its scene index), while a take there has not ended.
    pub fn recording_slot(&self, track: TrackId) -> Option<usize> {
        self.live.recording.takes.iter().find(|t| t.track == track && t.end.is_none()).and_then(|t| match t.target {
            Target::Slot { scene } => Some(scene),
            Target::Arrangement | Target::Sampler { .. } => None,
        })
    }

    /// Where the transport is: running, the launcher clock's beat, the arrangement's beat, the
    /// next launch boundary, the play start marker and the tempo. `None` without a player.
    fn clock(&self) -> Option<(bool, f64, f64, f64, f64)> {
        let monitor = self.player()?.monitor();
        Some((monitor.running(), monitor.beat(), monitor.launch_beat(), monitor.start_beat(), monitor.tempo_bpm()))
    }

    /// The file an audio take of `track` starting at `start` goes to.
    fn take_file(&self, track: TrackId, start: f64) -> Result<PathBuf, CommandError> {
        let folder = self
            .source()
            .and_then(std::path::Path::parent)
            .map(|f| f.join(RECORDED_FOLDER))
            .ok_or("save the project first: recorded audio is written to a folder next to the project file")?;
        let p = &self.project;
        let bar = p.transport.meter().position(start.max(0.0)).bar + 1;
        let base = crate::editing::sanitized(&track_name(p, track));
        let name = format!("{base} bar {bar}");
        // A file another take of this session is writing is taken too.
        for n in 1..=crate::editing::MAX_NAME_TRIES {
            let file = if n == 1 { format!("{name}.wav") } else { format!("{name} {n}.wav") };
            let path = folder.join(file);
            if !path.exists() && !self.live.recording.takes.iter().any(|t| t.file.as_ref() == Some(&path)) {
                return Ok(path);
            }
        }
        Err(format!("no free file name for {name:?} in {}", folder.display()).into())
    }

    /// Starts a take of each of `tracks` into `target` at `start`, turning the engine's recording
    /// on, and starts the transport (after the count-in) when it is stopped.
    fn begin_takes(
        &mut self,
        tracks: &[TrackId],
        target: Target,
        start: f64,
        running: bool,
    ) -> Result<(), CommandError> {
        let mut files = Vec::new();
        for &track in tracks {
            let audio = kind_of(&self.project, track).ok_or_else(|| {
                format!(
                    "track {:?} has nothing to record: no audio input and no notes",
                    track_name(&self.project, track)
                )
            })?;
            files.push(if audio { Some(self.take_file(track, start)?) } else { None });
        }
        let thread = self
            .live
            .recording
            .threads
            .last()
            .ok_or("the audio engine records nothing (it was opened without recording)")?;
        for (&track, file) in tracks.iter().zip(&files) {
            if let Some(path) = file {
                thread.send(Order::Start {
                    track,
                    path: path.clone(),
                    start,
                    by_clock: matches!(target, Target::Slot { .. }),
                })?;
            }
        }
        let player = self
            .sync
            .player
            .as_mut()
            .ok_or("no audio engine is attached; open the project in the app or with --audio")?;
        player.send(LaunchCommand::SetRecording { on: true })?;
        self.live.recording.engine_on = true;
        if !running {
            let bars = f64::from(self.live.recording.count_in_bars);
            let bar_beats = self.project.transport.meter().position(start.max(0.0)).bar_beats;
            player.send(LaunchCommand::CountIn { beats: bars * bar_beats, bar_beats })?;
        }
        for (&track, file) in tracks.iter().zip(files) {
            self.live.recording.takes.push(Take {
                track,
                target,
                file,
                start,
                end: None,
                asked: None,
                play: false,
                notes: Vec::new(),
                written: 0,
                end_sent: false,
                finished: None,
            });
        }
        Ok(())
    }

    /// Ends every take where the transport is now, none of them played: the transport stops or
    /// the engine is replaced.
    pub(crate) fn end_takes_now(&mut self) {
        let Some((_, clock, _, _, _)) = self.clock() else {
            for take in &mut self.live.recording.takes {
                take.end.get_or_insert(take.start);
                take.asked.get_or_insert_with(Instant::now);
            }
            self.live.recording.arrangement = false;
            self.send_ends();
            return;
        };
        for take in &mut self.live.recording.takes {
            let now = if take.end.is_some_and(|end| end < clock) { take.end } else { Some(clock) };
            take.end = now;
            take.asked = Some(Instant::now());
            take.play = false;
        }
        self.live.recording.arrangement = false;
        self.send_ends();
    }

    /// `clip.launch` on a slot that records: ends its take at the next launch boundary, and the
    /// new clip plays from there. Returns the answer, or `None` when the slot does not record.
    pub(crate) fn end_slot_take(&mut self, column: usize, scene: usize) -> Option<Value> {
        let track = self.project.launcher.columns.get(column)?.track;
        let boundary = self.clock().map(|(_, _, launch, _, _)| launch);
        let take = self
            .live
            .recording
            .takes
            .iter_mut()
            .find(|t| t.track == track && t.target == (Target::Slot { scene }) && t.end.is_none())?;
        let end = boundary.unwrap_or(take.start).max(take.start);
        take.end = Some(end);
        take.asked = Some(Instant::now());
        take.play = true;
        self.send_ends();
        Some(json!({ "scene": scene + 1, "queued": "end recording", "end_beat": end }))
    }

    /// Takes in what the recording thread has, finishes the takes that are done (one undo step
    /// each), turns the engine's recording off when nothing records, and asks slots whose new
    /// clip should play to play. Call it often: the window does every frame, through
    /// [`crate::Projects::poll_plugins`]. Returns what the user should hear about.
    pub fn poll_recording(&mut self) -> Vec<String> {
        for thread in &self.live.recording.threads {
            while let Some(report) = thread.report() {
                match report {
                    Report::Note(note) => {
                        if let Some(take) = self.live.recording.takes.iter_mut().find(|t| t.track == note.track) {
                            take.notes.push(note);
                        }
                    }
                    Report::Written { track, frames } => {
                        if let Some(take) = self.live.recording.takes.iter_mut().find(|t| t.track == track) {
                            take.written = frames;
                        }
                    }
                    Report::Finished { track, path, frames, channels, error } => {
                        if let Some(take) = self
                            .live
                            .recording
                            .takes
                            .iter_mut()
                            .find(|t| t.track == track && t.file.as_ref() == Some(&path))
                        {
                            take.written = frames;
                            take.finished = Some((frames, channels, error));
                        }
                    }
                }
            }
        }
        self.send_ends();
        let clock = self.clock();
        let running = clock.is_some_and(|c| c.0);
        let now_clock = clock.map_or(0.0, |c| c.1);
        // A slot take is past its end once the clock is; an arrangement take ends where the
        // transport is when asked. Either then waits a moment for notes on their way.
        let mut done = Vec::new();
        for (index, take) in self.live.recording.takes.iter_mut().enumerate() {
            let (Some(end), Some(asked)) = (take.end, take.asked) else { continue };
            let past_end = match take.target {
                // The clock adds up a step per frame: a boundary is reached within rounding.
                Target::Slot { .. } if running => now_clock >= end - 1e-6,
                _ => true,
            };
            if !past_end {
                take.asked = Some(Instant::now());
                continue;
            }
            if asked.elapsed() < SETTLE {
                continue;
            }
            if take.file.is_some() && take.finished.is_none() {
                continue;
            }
            done.push(index);
        }
        for index in done.into_iter().rev() {
            let take = self.live.recording.takes.remove(index);
            if let Err(e) = self.commit_take(take, now_clock) {
                self.live.recording.notices.push(e.0);
            }
        }
        if self.live.recording.takes.is_empty() && self.live.recording.engine_on {
            if let Some(player) = self.sync.player.as_mut() {
                let _ = player.send(LaunchCommand::SetRecording { on: false });
            }
            self.live.recording.engine_on = false;
        }
        self.resume_slots(now_clock);
        std::mem::take(&mut self.live.recording.notices)
    }

    /// Tells the recording thread where each audio take ends, as soon as that is known, so it
    /// writes nothing past it.
    fn send_ends(&mut self) {
        let threads = &self.live.recording.threads;
        for take in self.live.recording.takes.iter_mut().filter(|t| t.file.is_some() && !t.end_sent) {
            let Some(end) = take.end else { continue };
            take.end_sent = true;
            // Every thread hears it; only the one writing the take acts on it.
            for thread in threads {
                let _ = thread.send(Order::End { track: take.track, beats: end - take.start });
            }
        }
    }

    /// Asks the engine to play each slot whose take just became a clip, as if launched at its
    /// beat, until it does.
    fn resume_slots(&mut self, now_clock: f64) {
        let resumes = std::mem::take(&mut self.live.recording.resumes);
        let Some(player) = self.sync.player.as_mut() else { return };
        for (column, slot, at, until) in resumes {
            let monitor = player.monitor();
            let playing = monitor.column(column).and_then(|c| c.playing) == Some(slot);
            if playing || !monitor.running() || Instant::now() > until {
                continue;
            }
            let elapsed = (now_clock - at).max(0.0);
            if player.send(LaunchCommand::ResumeColumn { column, slot: Some(slot), elapsed_beats: elapsed }).is_ok() {
                self.live.recording.resumes.push((column, slot, at, until));
            }
        }
    }

    /// Makes `take` a clip: one undo step.
    fn commit_take(&mut self, take: Take, now_clock: f64) -> Result<(), CommandError> {
        if let Target::Sampler { device } = take.target {
            let path = take.file.as_ref().ok_or("sampler recording has no audio file")?;
            if take.finished.as_ref().is_none_or(|(frames, _, _)| *frames < 2) {
                return Err(format!("sample recording was empty: {}", path.display()).into());
            }
            self.execute(
                "device.loadSample",
                &json!({"track": take.track.0, "device": device, "path": path.display().to_string()}),
            )?;
            return Ok(());
        }
        let end = take.end.unwrap_or(take.start);
        let tempo = self.clock().map_or(self.project.transport.tempo_bpm, |c| c.4);
        let (lane, length) = match &take.file {
            None => {
                let mut length = end - take.start;
                if (length.is_nan() || length <= 0.0) && take.target == Target::Arrangement {
                    // The playhead looped back past the start: the clip spans what was played.
                    let field = |n: &RecordedNote| n.beat - take.start;
                    let last = take.notes.iter().map(field).fold(0.0, f64::max);
                    length = (last.ceil()).max(self.project.transport.meter().position(take.start.max(0.0)).bar_beats);
                }
                if !(length > 0.0 && length.is_finite()) {
                    return Ok(());
                }
                (LaneData::Notes(notes_of(&take, length)), length)
            }
            Some(path) => {
                let (frames, channels, error) = take.finished.clone().unwrap_or((0, 1, None));
                if let Some(error) = error {
                    self.live.recording.notices.push(error);
                }
                if frames == 0 {
                    self.live
                        .recording
                        .notices
                        .push(format!("nothing was recorded on {}", track_name(&self.project, take.track)));
                    let _ = std::fs::remove_file(path);
                    return Ok(());
                }
                let data = self
                    .decoded_audio(path)
                    .map_err(|e| format!("the recording {} could not be read back: {e}", path.display()))?;
                let rate = data.sample_rate.max(1);
                let seconds = frames as f64 / f64::from(rate);
                let length = seconds * tempo / 60.0;
                let key = crate::files::absolute(path).display().to_string();
                let region = AudioRegion {
                    file: key.clone(),
                    absolute_file: Some(key),
                    sample_rate: Some(rate),
                    channels: Some(channels),
                    duration_seconds: Some(seconds),
                    warped: false,
                    stretch: StretchMode::default(),
                    warp_markers: Vec::new(),
                    gain: 1.0,
                    transpose: 0.0,
                    stream: None,
                };
                (LaneData::Audio(region), length)
            }
        };
        let name = track_name(&self.project, take.track);
        let kind = if take.file.is_some() { TrackKind::Audio } else { TrackKind::Midi };
        let before = self.project.clone();
        let mut placed_slot = None;
        let result = (|| -> Result<(), CommandError> {
            let p = &mut self.project;
            let source = crate::files::Source { kind, name, length, lane, loops: true };
            match take.target {
                Target::Slot { scene } => {
                    let column = p
                        .launcher
                        .columns
                        .iter()
                        .position(|c| c.track == take.track)
                        .ok_or("the track has no clip slots")?;
                    let slots = &p.launcher.columns.get(column).ok_or("the track has no clip slots")?.slots;
                    // The slot it started in while still empty, else the first empty one below.
                    let row = (scene..slots.len())
                        .find(|&row| slots.get(row).is_some_and(|s| s.clip.is_none()))
                        .unwrap_or(p.launcher.scenes.len().max(slots.len()));
                    crate::files::place_clip(p, take.track, source, Some(row), 0.0, false)?;
                    if let Some(clip) = p
                        .launcher
                        .columns
                        .get_mut(column)
                        .and_then(|c| c.slots.get_mut(row))
                        .and_then(|s| s.clip.as_mut())
                    {
                        clip.loop_range = Some(LoopRange { start: 0.0, end: length });
                    }
                    placed_slot = Some((column, row));
                }
                Target::Sampler { .. } => return Err("sampler take must be committed as a source".into()),
                Target::Arrangement => {
                    crate::files::place_clip(p, take.track, source, None, take.start.max(0.0), false)?;
                }
            }
            // The take's clip, and the second part of a clip it split, get their ids (ADR 0025).
            p.assign_ids()?;
            p.validate().map_err(|errors| CommandError(format!("the take would leave an invalid project: {errors:?}")))
        })();
        if let Err(e) = result {
            self.project = before;
            return Err(e);
        }
        self.history.changes = self.history.changes.wrapping_add(1);
        self.push_undo(before);
        self.update_player(None)?;
        self.send_armed();
        if let (Some((column, row)), true) = (placed_slot, take.play) {
            let at = end;
            if now_clock >= at - 1e-6 {
                self.live.recording.resumes.push((column, row, at, Instant::now() + RESUME_FOR));
            }
        }
        Ok(())
    }
}

/// The notes of a MIDI take, in beats from its start, cut at `length`.
fn notes_of(take: &Take, length: f64) -> Vec<Note> {
    let by_clock = take.target != Target::Arrangement;
    let at = |n: &RecordedNote| if by_clock { n.clock } else { n.beat } - take.start;
    let mut open: [Option<(f64, f32)>; 128] = [None; 128];
    let mut notes = Vec::new();
    let close = |notes: &mut Vec<Note>, key: u8, (start, velocity): (f64, f32), end: f64| {
        if start >= 0.0 && start < length {
            notes.push(Note {
                time: start,
                duration: (end.min(length) - start).max(MIN_NOTE),
                key,
                velocity: velocity.clamp(1.0 / 127.0, 1.0),
                release_velocity: 0.5,
                channel: 0,
                muted: false,
            });
        }
    };
    for note in &take.notes {
        let Some(slot) = open.get_mut(usize::from(note.key)) else { continue };
        let beat = at(note);
        if !beat.is_finite() {
            continue;
        }
        match note.velocity {
            Some(velocity) => {
                if let Some(held) = slot.replace((beat, velocity)) {
                    close(&mut notes, note.key, held, beat);
                }
            }
            None => {
                if let Some(held) = slot.take() {
                    close(&mut notes, note.key, held, beat);
                }
            }
        }
    }
    for (key, held) in open.iter().enumerate() {
        if let (Some(held), Ok(key)) = (held, u8::try_from(key)) {
            close(&mut notes, key, *held, length);
        }
    }
    notes.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.key.cmp(&b.key)));
    notes
}

/// `clip.record`: records into an empty slot of an armed track from the next launch boundary.
pub(crate) fn record_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let p = session.project();
    let track = track_id(p, params)?;
    let column = column_index(p, params)?;
    let scene = scene_index(p, params)?;
    let slot = p.launcher.columns.get(column).and_then(|c| c.slots.get(scene)).ok_or("the slot is missing")?;
    if slot.clip.is_some() {
        return Err("that slot has a clip; recording needs an empty slot".into());
    }
    if !session.is_armed(track) {
        return Err(format!("arm track {:?} first (track.setArm)", track_name(p, track)).into());
    }
    if session.live.recording.take_on(track).is_some() {
        return Err(format!("track {:?} is recording already", track_name(p, track)).into());
    }
    let (running, _, launch, start_marker, _) =
        session.clock().ok_or("no audio engine is attached; open the project in the app or with --audio")?;
    let start = if running { launch } else { start_marker };
    session.begin_takes(&[track], Target::Slot { scene }, start, running)?;
    Ok(json!({
        "track": track.0,
        "scene": scene + 1,
        "start_beat": start,
        "count_in_bars": if running { 0 } else { session.live.recording.count_in_bars },
    }))
}

/// Record the track's selected audio input directly into its sampler source.
pub(crate) fn record_sample(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let track = track_id(session.project(), params)?;
    let device = params
        .get("device")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or("device must be an index")?;
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be a boolean")?;
    let (running, clock, launch, start_marker, _) =
        session.clock().ok_or("open an audio output before recording a sample")?;
    if !on {
        let take = session
            .live
            .recording
            .takes
            .iter_mut()
            .find(|t| t.track == track && t.target == Target::Sampler { device } && t.end.is_none())
            .ok_or("this sampler is not recording")?;
        take.end = Some(clock.max(take.start));
        take.asked = Some(Instant::now());
        session.send_ends();
        return Ok(json!({"recording": false}));
    }
    let t = session.project.track(track).ok_or("track is missing")?;
    let stored = t
        .channel
        .and_then(|id| session.project.channel(id))
        .and_then(|c| c.devices.get(device))
        .ok_or("device is missing")?;
    if !matches!(&stored.kind, suhina_model::DeviceKind::Builtin(id) if id == "sampler") {
        return Err("recordSample needs a Sampler instrument".into());
    }
    if kind_of(&session.project, track) != Some(true) {
        return Err("select an audio input on this track before recording a sample".into());
    }
    if !session.is_armed(track) {
        return Err("arm this track before recording a sample".into());
    }
    if session.live.recording.take_on(track).is_some() {
        return Err("this track already has a recording in progress".into());
    }
    let _ = launch;
    session.begin_takes(&[track], Target::Sampler { device }, if running { clock } else { start_marker }, running)?;
    Ok(json!({"recording": true}))
}

/// `transport.record`: records every armed track into the arrangement, or ends every take.
pub(crate) fn record_arrangement(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be true or false")?;
    let (running, clock, launch, start_marker, _) =
        session.clock().ok_or("no audio engine is attached; open the project in the app or with --audio")?;
    if !on {
        let mut ended = 0;
        for take in session.live.recording.takes.iter_mut().filter(|t| t.end.is_none()) {
            take.end = Some(match take.target {
                Target::Arrangement => clock,
                Target::Sampler { .. } => clock.max(take.start),
                Target::Slot { .. } if running => launch.max(take.start),
                Target::Slot { .. } => take.start,
            });
            take.asked = Some(Instant::now());
            take.play = matches!(take.target, Target::Slot { .. }) && running;
            ended += 1;
        }
        session.live.recording.arrangement = false;
        session.send_ends();
        return Ok(json!({ "recording": false, "ended": ended }));
    }
    if session.live.recording.arrangement {
        return Ok(json!({ "recording": true }));
    }
    let armed = session.armed();
    let tracks: Vec<TrackId> = armed.iter().copied().filter(|t| kind_of(session.project(), *t).is_some()).collect();
    if tracks.is_empty() {
        // Armed, but hearing no input: say which, so the input can be chosen (#682).
        if let Some(track) = armed.first() {
            return Err(format!(
                "track {:?} hears no audio input: choose one for it (track.setAudioInput), then record",
                track_name(session.project(), *track)
            )
            .into());
        }
        return Err("arm a track that has an audio input or plays notes first (track.setArm)".into());
    }
    if let Some(busy) = tracks.iter().find(|t| session.live.recording.take_on(**t).is_some()) {
        return Err(format!("track {:?} is recording into a slot already", track_name(session.project(), *busy)).into());
    }
    let start = if running { clock } else { start_marker };
    session.begin_takes(&tracks, Target::Arrangement, start, running)?;
    session.live.recording.arrangement = true;
    Ok(json!({
        "recording": true,
        "start_beat": start,
        "tracks": tracks.iter().map(|t| t.0).collect::<Vec<_>>(),
        "count_in_bars": if running { 0 } else { session.live.recording.count_in_bars },
    }))
}

/// `transport.setCountIn`.
pub(crate) fn set_count_in(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let bars = params
        .get("bars")
        .and_then(Value::as_u64)
        .and_then(|b| u8::try_from(b).ok())
        .filter(|b| COUNT_IN_BARS.contains(b))
        .ok_or("bars must be 0, 1, 2 or 4")?;
    session.live.recording.count_in_bars = bars;
    Ok(json!({ "count_in_bars": bars }))
}

/// `record.state`.
pub(crate) fn record_state(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let notices = session.poll_recording();
    let count_in = session.player().map_or(0.0, |p| p.monitor().count_in_beats());
    let p = session.project();
    let takes: Vec<Value> = session
        .live.recording
        .takes
        .iter()
        .map(|t| {
            json!({
                "track": t.track.0,
                "track_name": track_name(p, t.track),
                "into": match t.target { Target::Slot { scene } => json!({ "scene": scene + 1 }), Target::Arrangement => json!("arrangement"), Target::Sampler { device } => json!({"device": device}) },
                "kind": if t.file.is_some() { "audio" } else { "midi" },
                "start_beat": t.start,
                "end_beat": t.end,
                "notes": t.notes.iter().filter(|n| n.velocity.is_some()).count(),
                "frames": t.written,
                "file": t.file.as_ref().map(|f| f.display().to_string()),
            })
        })
        .collect();
    Ok(json!({
        "arrangement": session.live.recording.arrangement,
        "takes": takes,
        "count_in_bars": session.live.recording.count_in_bars,
        "count_in_left": (count_in * 1000.0).round() / 1000.0,
        "notices": notices,
    }))
}
