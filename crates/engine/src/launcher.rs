// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The clip launcher: one column of slots per track, at most one playing clip per column, and
//! launches that wait for the next quantisation boundary. Behaviour: `docs/spec/clip-launching.md`.
//!
//! Every launch clip is prepared when the [`Renderer`] is built; launching only changes indices
//! and counters, so [`Renderer::handle`] and the per-frame work never allocate. Other threads
//! send [`LaunchCommand`]s through a [`Controller`] (a lock-free queue) and read the state back
//! from a [`LauncherMonitor`] (atomics).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use suhina_model::{LaneData, Meter, Project, TempoMap, TimeSignature};

use crate::audio::{AudioEvent, AudioPool, Timing};
use crate::chain::Chain;
use crate::follow::{FollowPlan, Outcome, Random};
use crate::stretch::StretchVoice;
use crate::{AUDIO_HEADROOM, NoteStart, Renderer, bus_chain, note_velocity, route, start_note};

mod modes;
mod prepare;
use modes::SlotLaunch;
pub(crate) use prepare::Preparer;

/// Commands that can wait in the queue between two audio blocks.
const QUEUE_CAPACITY: usize = 256;
/// Fade-out of a stopped or replaced audio clip, in seconds.
const STOP_FADE_SECONDS: f64 = 0.005;
/// Length given to looping audio clips, in beats: long enough to be endless in practice.
const ENDLESS_BEATS: f64 = 10_000_000.0;
/// Length given to looping time-stretched clips, in beats (over an hour at 120 bpm); their
/// playlist is prepared up front, so it cannot be endless. After it they fall back to re-pitch.
const STRETCH_LOOP_BEATS: f64 = 8_192.0;
/// Monitor value for "no slot".
const NONE: u32 = u32::MAX;
/// Monitor value for "a stop is queued".
const STOP: u32 = u32::MAX - 1;
/// Columns a monitor reports at least: room for tracks added while the engine plays, since the
/// monitor that other threads hold is the first graph's (ADR 0005).
const MONITOR_COLUMNS: usize = 1024;
/// The latest beat the play start marker can be at: over 16 hours at 120 bpm.
pub const MAX_START_BEAT: f64 = 120_000.0;

/// A grid that launches wait for. Beats are quarter notes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quantization {
    None,
    Bars(u32),
    /// A note value, in beats: 1/16 is 0.25, a 1/8 triplet 1/3.
    Beats(f64),
}

impl Quantization {
    /// The grid step in beats, with bars of `bar_beats`; 0 means none.
    pub fn step(self, bar_beats: f64) -> f64 {
        let step = match self {
            Self::None => 0.0,
            Self::Bars(bars) => f64::from(bars) * bar_beats,
            Self::Beats(beats) => beats,
        };
        if step.is_finite() && step > 0.0 { step } else { 0.0 }
    }
}

/// An action for the launcher. Columns, slots and scenes are 0-based indices into
/// `Project::launcher`; out-of-range indices are ignored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LaunchCommand {
    /// Start the transport from the play start marker; nothing happens if it is running.
    Play,
    /// Stop the transport, every clip and the arrangement at once. The playhead goes back to the
    /// play start marker; when the transport is already stopped, the marker goes back to beat 0
    /// (`docs/spec/arrangement-editing.md` › Play position).
    Stop,
    /// Like [`LaunchCommand::Stop`], but the play start marker moves to where the playhead was,
    /// so the next start carries on from there.
    StopHere,
    /// Moves the play start marker to `beat` (non-finite values are ignored, others are kept
    /// within 0 … [`MAX_START_BEAT`]). While the transport runs, playback jumps there at once.
    SetPosition {
        beat: f64,
    },
    /// A press of a slot's launch button. What it does depends on the clip's launch mode
    /// (`docs/spec/clip-launching.md` › Launch settings): it launches the clip, or, for a clip in
    /// toggle mode that is playing, stops it.
    LaunchClip {
        column: usize,
        slot: usize,
    },
    /// The release of a slot's launch button: stops a clip in gate mode and ends the repeats of
    /// one in repeat mode. Ignored for the other modes, and for a slot that was not pressed.
    ReleaseClip {
        column: usize,
        slot: usize,
    },
    LaunchScene {
        scene: usize,
    },
    /// At the next quantisation point, every track whose slot in `scene`'s row has a clip stops.
    /// A track whose slot in the row is empty keeps playing.
    StopScene {
        scene: usize,
    },
    StopColumn {
        column: usize,
    },
    StopAll,
    SetQuantization(Quantization),
    /// Every track plays its arrangement clips again.
    BackToArrangement,
    /// Turns the metronome on or off; `gain` is its level (linear, 0.5 is -6 dB), or `None` to
    /// keep the level it has.
    SetMetronome {
        on: bool,
        gain: Option<f32>,
    },
    /// The metronome's click sound from the next click on.
    SetMetronomeSound(crate::ClickSound),
    /// While the transport runs, playback jumps to `beat` at once, as [`LaunchCommand::SetPosition`]
    /// does, but the play start marker stays where it is. Ignored while stopped (the playhead is
    /// at the marker then) and for non-finite beats. For an output that is opened again.
    Locate {
        beat: f64,
    },
    /// While the transport runs, `column`'s track plays launched clips from now on: the clip in
    /// `slot` as if it had been launched `elapsed_beats` ago (its notes and audio from there),
    /// or nothing when `slot` is `None`, as after a stop of that column. For an output that is
    /// opened again; ignored while stopped, and for a slot without a clip that can play.
    ResumeColumn {
        column: usize,
        slot: Option<usize>,
        elapsed_beats: f64,
    },
    /// Sets the launcher's tempo at once, as a scene's tempo does, taking the clock off the
    /// tempo map (1 to 999 bpm; other values are ignored). For an output that is opened again.
    SetLaunchTempo {
        bpm: f64,
    },
    /// Puts the launcher clock back on the project's tempo map after a scene's tempo or time
    /// signature took it off: the tempo and the bars are the map's again from now on, and a
    /// scene tempo or time signature still waiting for its beat is dropped.
    FollowTempoMap,
    /// Sets the cue volume (`gain`, linear, 0 to `MAX_CUE_GAIN`) and whether the cue has an
    /// output pair of its own (`separate`: the preview is then not heard on the main output),
    /// each when given.
    SetCue {
        gain: Option<f32>,
        separate: Option<bool>,
    },
    /// Seeds the random numbers that follow actions are picked with; the same seed picks the
    /// same actions in the same playback.
    SeedRandom(u64),
    /// Arms or disarms a track: an armed track's instrument plays the notes of the live input
    /// that `input` takes (ADR 0017). Arming an armed track changes what it hears. Disarming
    /// lets go of the keys held down.
    SetArmed {
        track: suhina_model::TrackId,
        armed: bool,
        input: crate::live::LiveFilter,
    },
    /// Atomically sets independent recording arm and live MIDI monitoring (ADR 0017).
    SetMidiInput {
        track: suhina_model::TrackId,
        armed: bool,
        monitoring: bool,
        input: crate::live::LiveFilter,
    },
    /// Turns recording on or off: while on and the transport runs, what the armed tracks play
    /// goes out through the recording rings ([`Controller::take_recorder`],
    /// `docs/spec/recording.md`).
    SetRecording {
        on: bool,
    },
    /// With the transport stopped, counts `beats` beats (up to 64, bars of `bar_beats`) with
    /// the metronome's click, then starts the transport; with nothing to count, starts it at
    /// once. Ignored while running. A stop cancels the count and leaves the transport stopped.
    CountIn {
        beats: f64,
        bar_beats: f64,
    },
}

/// The bars playing, as [`LauncherMonitor::meter`] reports them: the project's meter while the
/// clock follows the tempo map, or a time signature a scene took it off the map with, its bars
/// counted from where it changed (`docs/spec/tempo-map.md` › Scenes and the tempo map).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayingMeter {
    /// The clock follows the tempo map: bars are the project meter's, and the fields below are
    /// not used.
    pub follows_map: bool,
    pub time_signature: TimeSignature,
    /// The beat this time signature's bars are counted from.
    pub origin: f64,
    /// Bars played before `origin`, a bar cut short by the change counted as a whole one.
    pub bars_before: f64,
}

impl Default for PlayingMeter {
    /// The project's meter.
    fn default() -> Self {
        Self {
            follows_map: true,
            time_signature: TimeSignature { numerator: 4, denominator: 4 },
            origin: 0.0,
            bars_before: 0.0,
        }
    }
}

impl PlayingMeter {
    /// Beats in one bar of the time signature a scene set: a 4/4 bar has 4, a 6/8 bar 3.
    pub fn bar_beats(&self) -> f64 {
        bar_beats(self.time_signature)
    }

    /// The bar `beat` is in, counted from 1, and the beats into that bar; `project` is the
    /// project's meter, which counts the bars while the clock follows the tempo map.
    pub fn locate(&self, beat: f64, project: &Meter) -> (u64, f64) {
        let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
        if self.follows_map {
            let here = project.position(beat);
            return (u64::from(here.bar) + 1, (beat - here.bar_start).max(0.0));
        }
        let origin = if self.origin.is_finite() { self.origin.clamp(0.0, beat) } else { 0.0 };
        let bars_before = if self.bars_before.is_finite() { self.bars_before.max(0.0) } else { 0.0 };
        let bar_beats = self.bar_beats();
        let into = beat - origin;
        ((bars_before + (into / bar_beats).floor()) as u64 + 1, into % bar_beats)
    }
}

/// How the scenes of a new graph line up with the playing graph's when one scene was inserted
/// or removed, so that what each column plays and has queued stays with its clip across the
/// swap (ADR 0005). Without one, slots carry over by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneShift {
    /// A scene was inserted at this index: the slots from it on moved one row down.
    Inserted(usize),
    /// The scene at this index was removed with its slots: later slots moved one row up.
    Removed(usize),
}

impl SceneShift {
    /// Where the slot at `slot` before the shift is after it; `None` for a removed slot.
    fn map(self, slot: usize) -> Option<usize> {
        match self {
            Self::Inserted(at) if slot >= at => slot.checked_add(1),
            Self::Removed(at) if slot == at => None,
            Self::Removed(at) if slot > at => Some(slot - 1),
            _ => Some(slot),
        }
    }

    /// The shift that turns launcher `before` into `after`, when they differ by exactly one
    /// inserted or removed scene (its row in every column) and nothing else. Where neighbouring
    /// rows are equal, the later index is taken, so rows above an inserted duplicate keep theirs.
    pub fn between(before: &suhina_model::Launcher, after: &suhina_model::Launcher) -> Option<Self> {
        let (short, long, inserted) = match after.scenes.len().checked_sub(before.scenes.len()) {
            Some(1) => (before, after, true),
            _ if before.scenes.len().checked_sub(after.scenes.len()) == Some(1) => (after, before, false),
            _ => return None,
        };
        if short.columns.len() != long.columns.len()
            || short.columns.iter().zip(&long.columns).any(|(a, b)| a.track != b.track)
        {
            return None;
        }
        let same_row = |i: usize, j: usize| {
            short.scenes.get(i) == long.scenes.get(j)
                && short.columns.iter().zip(&long.columns).all(|(a, b)| a.slots.get(i) == b.slots.get(j))
        };
        let rows = short.scenes.len();
        let prefix = (0..rows).take_while(|&i| same_row(i, i)).count();
        let suffix = (0..rows).take_while(|&k| same_row(rows - 1 - k, rows - k)).count();
        if prefix + suffix < rows {
            return None;
        }
        Some(if inserted { Self::Inserted(prefix) } else { Self::Removed(prefix) })
    }
}

/// What a column is waiting to do, as reported by [`LauncherMonitor::column`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queued {
    Launch(usize),
    Stop,
}

/// One column's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnStatus {
    /// The slot whose clip is playing.
    pub playing: Option<usize>,
    pub queued: Option<Queued>,
    /// The track plays launched clips instead of its arrangement clips.
    pub overrides_arrangement: bool,
}

/// The launcher's state, written by the audio thread and readable from any thread.
#[derive(Debug)]
pub struct LauncherMonitor {
    pub(crate) running: AtomicBool,
    beat: AtomicU64,
    tempo: AtomicU64,
    /// Columns the playing graph has (at most the room below), written after their states.
    columns: AtomicUsize,
    playing: Vec<AtomicU32>,
    queued: Vec<AtomicU32>,
    overrides: Vec<AtomicBool>,
    /// Beats each column's playing clip has played since its launch (f64 bits), as of the
    /// start of the last block.
    elapsed: Vec<AtomicU64>,
    /// Renderer frames played so far, and the arrangement frame playing (ADR 0005).
    pub(crate) position: AtomicU64,
    pub(crate) arrangement: AtomicU64,
    pub(crate) stretch_underruns: AtomicU64,
    /// Graph swaps while playing that had to move the stretch voices (they landed too far from
    /// where they were rendered for): each silences the stretched clips until they catch up.
    pub(crate) stretch_moves: AtomicU64,
    /// Plug-in blocks the playing graph's plug-ins did not answer in time, and the plug-in
    /// (its key) that missed most with its count: what a crackle with no device underrun is.
    pub(crate) plugin_misses: AtomicU64,
    pub(crate) worst_plugin: AtomicU64,
    pub(crate) worst_misses: AtomicU64,
    /// Arrangement frames per beat of the playing graph (f64 bits), so an update can find the
    /// playhead's beat.
    pub(crate) frames_per_beat: AtomicU64,
    metronome: AtomicBool,
    /// The play start marker, in beats (f64 bits).
    pub(crate) start_beat: AtomicU64,
    /// The time signature playing, as `numerator << 8 | denominator`.
    signature: AtomicU32,
    /// The beat bars are counted from (f64 bits): where the time signature last changed.
    bar_origin: AtomicU64,
    /// Bars played before `bar_origin` (f64 bits).
    bars_before: AtomicU64,
    /// The clock follows the tempo map (no scene has taken it off).
    follows_map: AtomicBool,
    /// The beat a launch issued now happens at, on the global grid (f64 bits).
    launch_beat: AtomicU64,
    /// Beats the count-in still counts before the transport starts (f64 bits); 0 when none.
    pub(crate) count_in: AtomicU64,
}

impl LauncherMonitor {
    fn new(columns: usize, tempo: f64, signature: TimeSignature) -> Self {
        let room = columns.max(MONITOR_COLUMNS);
        Self {
            running: AtomicBool::new(false),
            beat: AtomicU64::new(0f64.to_bits()),
            tempo: AtomicU64::new(tempo.to_bits()),
            columns: AtomicUsize::new(columns),
            playing: (0..room).map(|_| AtomicU32::new(NONE)).collect(),
            queued: (0..room).map(|_| AtomicU32::new(NONE)).collect(),
            overrides: (0..room).map(|_| AtomicBool::new(false)).collect(),
            elapsed: (0..room).map(|_| AtomicU64::new(0f64.to_bits())).collect(),
            position: AtomicU64::new(0),
            arrangement: AtomicU64::new(0),
            stretch_underruns: AtomicU64::new(0),
            stretch_moves: AtomicU64::new(0),
            plugin_misses: AtomicU64::new(0),
            worst_plugin: AtomicU64::new(0),
            worst_misses: AtomicU64::new(0),
            frames_per_beat: AtomicU64::new(0),
            metronome: AtomicBool::new(false),
            start_beat: AtomicU64::new(0f64.to_bits()),
            signature: AtomicU32::new(pack(signature)),
            bar_origin: AtomicU64::new(0f64.to_bits()),
            bars_before: AtomicU64::new(0f64.to_bits()),
            follows_map: AtomicBool::new(true),
            launch_beat: AtomicU64::new(0f64.to_bits()),
            count_in: AtomicU64::new(0f64.to_bits()),
        }
    }

    /// The beat a launch issued now would happen at, on the global launch grid, as of the start
    /// of the last block: where a recording into a slot starts or ends.
    pub fn launch_beat(&self) -> f64 {
        f64::from_bits(self.launch_beat.load(Ordering::Relaxed))
    }

    /// Beats the count-in still counts before the transport starts; 0 when none counts.
    pub fn count_in_beats(&self) -> f64 {
        f64::from_bits(self.count_in.load(Ordering::Relaxed))
    }

    /// Whether the transport is running.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// The playhead, in beats: where the transport started (the play start marker) plus the
    /// beats played since. While stopped it is at the marker.
    pub fn beat(&self) -> f64 {
        f64::from_bits(self.beat.load(Ordering::Relaxed))
    }

    /// Where the transport starts from, in beats: the play start marker.
    pub fn start_beat(&self) -> f64 {
        f64::from_bits(self.start_beat.load(Ordering::Relaxed))
    }

    pub fn tempo_bpm(&self) -> f64 {
        f64::from_bits(self.tempo.load(Ordering::Relaxed))
    }

    /// The bars playing: the project's meter while the clock follows the tempo map, or the
    /// time signature a scene set and where its bars start.
    pub fn meter(&self) -> PlayingMeter {
        let packed = self.signature.load(Ordering::Relaxed);
        PlayingMeter {
            follows_map: self.follows_map.load(Ordering::Relaxed),
            time_signature: TimeSignature { numerator: (packed >> 8) as u8, denominator: packed as u8 },
            origin: f64::from_bits(self.bar_origin.load(Ordering::Relaxed)),
            bars_before: f64::from_bits(self.bars_before.load(Ordering::Relaxed)),
        }
    }

    /// Whether the metronome clicks while the transport runs.
    pub fn metronome(&self) -> bool {
        self.metronome.load(Ordering::Relaxed)
    }

    pub fn columns(&self) -> usize {
        self.columns.load(Ordering::Acquire).min(self.playing.len())
    }

    pub fn column(&self, column: usize) -> Option<ColumnStatus> {
        if column >= self.columns() {
            return None;
        }
        let slot = |v: u32| (v < STOP).then_some(v as usize);
        let playing = slot(self.playing.get(column)?.load(Ordering::Relaxed));
        let queued = match self.queued.get(column)?.load(Ordering::Relaxed) {
            NONE => None,
            STOP => Some(Queued::Stop),
            v => Some(Queued::Launch(v as usize)),
        };
        let overrides_arrangement = self.overrides.get(column)?.load(Ordering::Relaxed);
        Some(ColumnStatus { playing, queued, overrides_arrangement })
    }

    /// Beats `column`'s playing clip has played since it was launched, as of the start of the
    /// last audio block; `None` while nothing plays there.
    pub fn elapsed_beats(&self, column: usize) -> Option<f64> {
        self.column(column)?.playing?;
        Some(f64::from_bits(self.elapsed.get(column)?.load(Ordering::Relaxed)))
    }
}

/// Sends commands to a renderer that may be running on the audio thread.
pub struct Controller {
    producer: rtrb::Producer<LaunchCommand>,
    pub(crate) monitor: Arc<LauncherMonitor>,
    pub(crate) updater: crate::swap::Updater,
    pub(crate) preview: crate::preview::PreviewSender,
    pub(crate) preview_monitor: Arc<crate::preview::PreviewMonitor>,
    pub(crate) cpu_monitor: Arc<crate::cpu::CpuMonitor>,
    pub(crate) meter_monitor: Arc<crate::meters::MeterMonitor>,
    /// The live-note queue's sending end, until taken for the MIDI input (ADR 0017).
    live: Option<crate::live::LiveSender>,
    /// The recording rings' reading end, until taken (`docs/spec/recording.md`).
    recorder: Option<crate::record::RecordReceiver>,
}

impl Controller {
    /// The reading end of what the renderer records ([`LaunchCommand::SetRecording`]); `None`
    /// once taken.
    pub fn take_recorder(&mut self) -> Option<crate::record::RecordReceiver> {
        self.recorder.take()
    }

    /// The sending end of the renderer's live-note queue, for the thread a MIDI driver calls
    /// back on; `None` once taken.
    pub fn take_live_input(&mut self) -> Option<crate::live::LiveSender> {
        self.live.take()
    }

    /// Queues a command for the start of the next audio block. Fails only if the queue is full,
    /// which means the renderer is not running.
    pub fn send(&mut self, command: LaunchCommand) -> Result<(), String> {
        self.producer.push(command).map_err(|_| "the audio engine is not taking commands (queue full)".to_string())
    }

    pub fn monitor(&self) -> &LauncherMonitor {
        &self.monitor
    }

    /// The audio thread's CPU load: the smoothed share of wall time it spends rendering, in
    /// per mille (1000 is busy all of the time).
    pub fn cpu_monitor(&self) -> &crate::cpu::CpuMonitor {
        &self.cpu_monitor
    }

    /// The level meters of the main output and every track.
    pub fn meter_monitor(&self) -> &crate::meters::MeterMonitor {
        &self.meter_monitor
    }

    /// The level meters' monitor by its own arc, so a front end can keep it in frame data
    /// (the EQ panel's analyser, docs/spec/eq.md › The panel › The analyser).
    pub fn meter_monitor_arc(&self) -> Arc<crate::meters::MeterMonitor> {
        Arc::clone(&self.meter_monitor)
    }
}

/// A note of a launch clip, in content beats, with its mix resolved.
#[derive(Debug, Clone, Copy)]
struct PlanNote {
    time: f64,
    duration: f64,
    frequency: f64,
    key: u8,
    velocity: f32,
    track: suhina_model::TrackId,
    gain_left: f32,
    gain_right: f32,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
    /// The track's meter slot.
    meter: u32,
    /// The scale of the clip.
    scale: Option<suhina_device_api::MusicalScale>,
}

struct PlanAudio {
    /// Placed at frame 0 at the project tempo; reads the file directly (re-pitch).
    event: AudioEvent,
    /// Warped audio follows the launcher's tempo; unwarped audio plays at file speed.
    follows_tempo: bool,
    /// The time-stretched voice of a warped clip, used while the tempo is the project tempo it
    /// was prepared for (ADR 0003). Built off the audio thread ([`prepare`]); silent until then.
    stretch: Option<StretchVoice>,
    /// The voice's number among the launcher's voices ([`prepare::Voices`]).
    voice: Option<usize>,
    /// Beats the stretch voice covers; after that the clip is re-pitched.
    stretch_beats: f64,
    stretch_gains: (f32, f32),
    /// Identifies what the stretch voice plays (region, placement, tempo, sample rate, the
    /// decoded audio), so a graph swap can keep a voice whose audio has not changed (ADR 0005).
    fingerprint: u64,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
    /// The track's meter slot.
    meter: u32,
}

/// A slot's clip, ready to play.
struct ClipPlan {
    offset: f64,
    length: f64,
    loop_range: Option<(f64, f64)>,
    /// Sorted by time.
    notes: Vec<PlanNote>,
    audio: Vec<PlanAudio>,
    follow: Option<FollowPlan>,
}

impl ClipPlan {
    /// The content beat reached `elapsed` beats after launch.
    fn clip_beat(&self, elapsed: f64) -> f64 {
        let beat = self.offset + elapsed;
        match self.loop_range {
            Some((start, end)) if beat >= end => start + (beat - end).rem_euclid(end - start),
            _ => beat,
        }
    }

    /// Notes end here: the loop end, or the end marker.
    fn note_limit(&self) -> f64 {
        self.loop_range.map_or(self.offset + self.length, |(_, end)| end)
    }

    fn first_note_from(&self, beat: f64) -> usize {
        self.notes.partition_point(|n| n.time < beat)
    }
}

struct SlotPlan {
    clip: Option<ClipPlan>,
    launch: SlotLaunch,
    /// The slot holds a clip, even one that cannot play (deactivated or unplannable): launching
    /// it does nothing, rather than what an empty slot's stop button does.
    has_clip: bool,
    has_stop: bool,
}

/// A clip that is playing or fading out.
#[derive(Debug, Clone, Copy)]
struct Playing {
    slot: usize,
    elapsed_beats: f64,
    elapsed_frames: f64,
    last_clip_beat: f64,
    next_note: usize,
    /// Frames of fade-out left, while stopping.
    fade_left: f64,
    /// Whether this playback uses the clip's stretch voices; it stops using them for good when
    /// the tempo changes.
    stretched: bool,
    /// The clip's follow action has run (or was skipped) for this launch.
    followed: bool,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    at_beat: f64,
    action: Queued,
    /// The next launch of a clip in repeat mode while its button is held, which the release
    /// takes back.
    repeat: bool,
}

struct Column {
    slots: Vec<SlotPlan>,
    playing: Option<Playing>,
    fading: Option<Playing>,
    pending: Option<Pending>,
    /// The slot whose launch button is held down, when its clip listens for the release (gate
    /// and repeat modes).
    held: Option<usize>,
    /// The frame from which the track plays launched clips instead of its arrangement.
    overrides_since: Option<u64>,
}

pub(crate) struct Launcher {
    columns: Vec<Column>,
    scene_tempos: Vec<Option<f64>>,
    scene_signatures: Vec<Option<TimeSignature>>,
    quantization: Quantization,
    /// The time signature playing, and the project's, which the playing one follows when it is
    /// edited.
    time_signature: TimeSignature,
    project_signature: TimeSignature,
    bar_beats: f64,
    /// The beat bars are counted from: where a scene last changed the time signature.
    bar_origin: f64,
    /// Bars played before `bar_origin`.
    bars_before: f64,
    /// A scene's time signature waiting for its launch beat.
    pending_signature: Option<(f64, TimeSignature)>,
    beat: f64,
    tempo_bpm: f64,
    /// The project's start tempo, which stretch voices are prepared for.
    project_tempo_bpm: f64,
    /// The arrangement's tempo map in frames and its meter (`docs/spec/tempo-map.md`).
    map: Arc<TempoMap>,
    meter: Meter,
    /// The clock runs on the map; a scene tempo takes it off the map until the map is edited.
    follows_map: bool,
    pending_tempo: Option<(f64, f64)>,
    frames_per_beat_at_build: f64,
    stop_fade_frames: f64,
    pub(crate) monitor: Arc<LauncherMonitor>,
    control: Option<rtrb::Consumer<LaunchCommand>>,
    random: Random,
    /// The stretch voices of the slots' clips and their preparation off the audio thread.
    voices: prepare::Voices,
}

impl Launcher {
    pub(crate) fn new(
        project: &Project,
        audio: &AudioPool,
        sample_rate: u32,
        chains: &[Chain],
        map: Arc<TempoMap>,
    ) -> Self {
        let tempo = project.transport.tempo_bpm;
        let frames_per_beat = 60.0 / tempo * f64::from(sample_rate);
        let meter = project.transport.meter();
        let mut columns: Vec<Column> = project
            .launcher
            .columns
            .iter()
            .map(|column| Column {
                slots: column
                    .slots
                    .iter()
                    .map(|slot| SlotPlan {
                        clip: slot.clip.as_ref().filter(|c| c.enabled).and_then(|c| {
                            plan_clip(project, audio, chains, c, column.track, frames_per_beat, sample_rate)
                        }),
                        launch: slot.clip.as_ref().map(|c| SlotLaunch::new(&c.launch)).unwrap_or_default(),
                        has_clip: slot.clip.is_some(),
                        has_stop: slot.has_stop,
                    })
                    .collect(),
                playing: None,
                fading: None,
                pending: None,
                held: None,
                overrides_since: None,
            })
            .collect();
        // Every stretch voice gets its number; they are built later, off the audio thread.
        let mut voices = Vec::new();
        for (c, column) in columns.iter_mut().enumerate() {
            for (s, slot) in column.slots.iter_mut().enumerate() {
                for (i, a) in slot.clip.iter_mut().flat_map(|plan| plan.audio.iter_mut()).enumerate() {
                    if let Some(voice) = a.stretch.as_ref() {
                        a.voice = Some(voices.len());
                        voices.push((prepare::Place { column: c, slot: s, audio: i }, voice.spec(), a.fingerprint));
                    }
                }
            }
        }
        Self {
            monitor: Arc::new(LauncherMonitor::new(columns.len(), tempo, project.transport.time_signature)),
            columns,
            scene_tempos: project
                .launcher
                .scenes
                .iter()
                .map(|s| s.tempo_bpm.filter(|bpm| bpm.is_finite() && (1.0..=999.0).contains(bpm)))
                .collect(),
            scene_signatures: project
                .launcher
                .scenes
                .iter()
                .map(|s| s.time_signature.filter(|ts| ts.numerator > 0 && ts.denominator > 0))
                .collect(),
            quantization: Quantization::Bars(1),
            time_signature: project.transport.time_signature,
            project_signature: project.transport.time_signature,
            bar_beats: meter.start_bar_beats(),
            bar_origin: 0.0,
            bars_before: 0.0,
            pending_signature: None,
            meter,
            map,
            follows_map: true,
            beat: 0.0,
            tempo_bpm: tempo,
            project_tempo_bpm: tempo,
            pending_tempo: None,
            frames_per_beat_at_build: frames_per_beat,
            stop_fade_frames: (STOP_FADE_SECONDS * f64::from(sample_rate)).max(1.0),
            control: None,
            random: Random::default(),
            voices: prepare::Voices::new(voices),
        }
    }

    /// Takes over `old`'s runtime state at a graph swap (ADR 0005): what each column plays, fades
    /// and has queued, the beat, tempo and quantisation, and the control queue and monitor (the
    /// new graph's own go back with the old graph). A slot whose stretched audio has not changed
    /// keeps its prepared stretch voice ([`Launcher::take_voices`]); any other playing stretch
    /// voice is moved to where its clip is. Slots carry over by index, or along `shift` when a
    /// scene was inserted or removed. A column or slot that no longer exists is dropped. Never
    /// allocates.
    pub(crate) fn adopt(&mut self, old: &mut Launcher, shift: Option<SceneShift>) {
        std::mem::swap(&mut self.monitor, &mut old.monitor);
        std::mem::swap(&mut self.control, &mut old.control);
        self.quantization = old.quantization;
        self.random = old.random;
        self.beat = old.beat;
        self.pending_tempo = old.pending_tempo;
        // An edited tempo map or meter puts the clock back on the map; otherwise a scene's tempo
        // and time signature stay, with their bars.
        self.pending_signature = old.pending_signature;
        if *self.map == *old.map && self.meter == old.meter {
            self.tempo_bpm = old.tempo_bpm;
            self.follows_map = old.follows_map;
            self.time_signature = old.time_signature;
            self.bar_beats = old.bar_beats;
            self.bar_origin = old.bar_origin;
            self.bars_before = old.bars_before;
        } else {
            self.tempo_bpm = self.map.bpm_at(self.beat);
            self.follows_map = true;
        }
        let frames_per_beat = self.frames_per_beat_at_build;
        let map = |slot: usize| shift.map_or(Some(slot), |s| s.map(slot));
        self.take_voices(old, map);
        let voices = &self.voices;
        for (new, old) in self.columns.iter_mut().zip(old.columns.iter_mut()) {
            new.overrides_since = old.overrides_since;
            new.held = old.held;
            new.pending = old.pending.and_then(|p| match p.action {
                Queued::Launch(slot) => map(slot)
                    .filter(|&slot| new.slots.get(slot).is_some_and(|s| s.clip.is_some()))
                    .map(|slot| Pending { action: Queued::Launch(slot), ..p }),
                Queued::Stop => Some(p),
            });
            new.playing = carry(&mut new.slots, &old.slots, old.playing, map, frames_per_beat, voices);
            new.fading = carry(&mut new.slots, &old.slots, old.fading, map, frames_per_beat, voices);
        }
        // The monitor is the old graph's: report this graph's columns, added ones included.
        self.publish();
    }

    /// Whether the track behind `column` plays launched clips instead of its arrangement.
    pub(crate) fn overrides(&self, column: u32) -> bool {
        self.columns.get(column as usize).is_some_and(|c| c.overrides_since.is_some())
    }

    /// Like [`Launcher::overrides`], at frame `position` of the block just rendered.
    pub(crate) fn overrides_at(&self, column: u32, position: u64) -> bool {
        self.columns.get(column as usize).and_then(|c| c.overrides_since).is_some_and(|since| since <= position)
    }

    /// The tempo playing now.
    pub(crate) fn tempo_bpm(&self) -> f64 {
        self.tempo_bpm
    }

    /// The beat playing now.
    pub(crate) fn beat(&self) -> f64 {
        self.beat
    }

    /// Whether beat number `index` (counted from beat 0) starts a bar, for the metronome:
    /// through the project's meter while the clock follows the tempo map, otherwise through the
    /// time signature playing, its bars counted from where it last changed.
    pub(crate) fn starts_bar(&self, index: i64) -> bool {
        if self.follows_map {
            return crate::tempo::starts_bar(&self.meter, index);
        }
        let into_bar = (index as f64 - self.bar_origin).rem_euclid(self.bar_beats);
        self.bar_beats > 0.0 && (into_bar < 1e-9 || self.bar_beats - into_bar < 1e-9)
    }

    /// Whether the clock reads its beat from the tempo map: it follows the map and the map
    /// changes (a constant map is counted frame by frame, as it always was).
    fn on_map(&self) -> bool {
        self.follows_map && !self.map.is_constant()
    }

    /// Puts the tempo at the clock's beat, when the clock follows the map.
    fn tempo_from_map(&mut self) {
        if self.follows_map {
            self.tempo_bpm = self.map.bpm_at(self.beat);
        }
    }

    /// Takes the clock off the tempo map, as a scene's tempo or time signature does: the tempo
    /// and the bar it is in stay as the map has them here, and the bars from here on are counted
    /// in that bar's time signature until a scene changes it
    /// (`docs/spec/tempo-map.md` › Scenes and the tempo map). Never allocates.
    fn leave_map(&mut self) {
        if !self.follows_map {
            return;
        }
        let here = self.meter.position(self.beat.max(0.0) + 1e-9);
        self.time_signature = here.time_signature;
        self.bar_beats = here.bar_beats;
        self.bar_origin = here.bar_start;
        self.bars_before = f64::from(here.bar);
        self.follows_map = false;
    }

    /// The bar the clock is in: its number from 0, the beat it starts at and its time
    /// signature, the map's or the one a scene set. Never allocates.
    pub(crate) fn bar(&self) -> (u32, f64, TimeSignature) {
        let beat = if self.beat.is_finite() { self.beat.max(0.0) } else { 0.0 };
        if self.follows_map {
            let here = self.meter.position(beat + 1e-9);
            return (here.bar, here.bar_start, here.time_signature);
        }
        // Signed: a jump back before where the scene set the bars (a locate) is in a bar before
        // it, which starts before the playhead, not at `bar_origin` after it.
        let into = beat - self.bar_origin;
        let bars = if self.bar_beats > 0.0 && into.is_finite() { (into / self.bar_beats + 1e-9).floor() } else { 0.0 };
        let number = (self.bars_before + bars).clamp(0.0, f64::from(u32::MAX)) as u32;
        (number, (self.bar_origin + bars * self.bar_beats).max(0.0), self.time_signature)
    }

    /// Whether the clock follows the tempo map (no scene has taken it off).
    pub(crate) fn follows_map(&self) -> bool {
        self.follows_map
    }

    /// Puts the clock back on the tempo map: the tempo and the bars are the map's again, and a
    /// scene's tempo or time signature still waiting for its beat is dropped. Never allocates.
    fn follow_map(&mut self) {
        self.follows_map = true;
        self.pending_tempo = None;
        self.pending_signature = None;
        self.time_signature = self.project_signature;
        self.bar_beats = self.meter.start_bar_beats();
        self.bar_origin = 0.0;
        self.bars_before = 0.0;
        self.tempo_from_map();
    }

    /// The beat a launch issued now happens at. Bars are counted from where the time signature
    /// last changed; note values from beat 0.
    fn launch_beat(&self) -> f64 {
        self.launch_beat_on(self.quantization)
    }

    /// The beat a launch issued now on the grid `quantization` happens at.
    fn launch_beat_on(&self, quantization: Quantization) -> f64 {
        if self.follows_map && !self.meter.is_constant() {
            return self.launch_beat_on_meter(quantization);
        }
        let step = quantization.step(self.bar_beats);
        if step == 0.0 {
            return self.beat;
        }
        let origin = if matches!(quantization, Quantization::Bars(_)) { self.bar_origin } else { 0.0 };
        // A launch exactly on a boundary (within rounding) happens there.
        let index = ((self.beat - origin - 1e-9) / step).ceil();
        (origin + index * step).max(0.0)
    }

    /// [`Launcher::launch_beat_on`] when the time signature changes: bars are the meter's, and a
    /// note value counts from the start of the bar it is in.
    fn launch_beat_on_meter(&self, quantization: Quantization) -> f64 {
        let beat = self.beat.max(0.0);
        // The bar the beat is in; a beat a rounding error before a bar line counts as on it, and
        // a launch exactly on a boundary happens there.
        let here = self.meter.position(beat + 1e-9);
        match quantization {
            Quantization::Bars(bars) if bars > 0 => {
                let next = if (here.bar_start - beat).abs() <= 1e-9 { here.bar } else { here.bar.saturating_add(1) };
                self.meter.bar_start(next.div_ceil(bars).saturating_mul(bars))
            }
            Quantization::Beats(step) if step.is_finite() && step > 0.0 => {
                let index = ((beat - here.bar_start - 1e-9) / step).ceil().max(0.0);
                // A grid that does not divide the bar ends at the next bar line.
                (here.bar_start + index * step).min(here.bar_start + here.bar_beats)
            }
            _ => self.beat,
        }
    }

    /// Queues `action` for the next boundary: a clip's launch waits for the clip's own
    /// quantisation, when it has one, a stop for the global one.
    fn queue(&mut self, column: usize, action: Queued) {
        let quantization = match action {
            Queued::Launch(slot) => self.slot_quantization(column, slot),
            Queued::Stop => self.quantization,
        };
        self.queue_on(column, action, quantization);
    }

    fn queue_on(&mut self, column: usize, action: Queued, quantization: Quantization) {
        let at_beat = self.launch_beat_on(quantization);
        if let Some(c) = self.columns.get_mut(column) {
            c.pending = Some(Pending { at_beat, action, repeat: false });
        }
    }

    fn publish_meter(&self) {
        let m = &self.monitor;
        m.signature.store(pack(self.time_signature), Ordering::Relaxed);
        m.bar_origin.store(self.bar_origin.to_bits(), Ordering::Relaxed);
        m.bars_before.store(self.bars_before.to_bits(), Ordering::Relaxed);
        m.follows_map.store(self.follows_map, Ordering::Relaxed);
    }

    /// Queues, for this frame, the follow action of each playing clip whose follow time has
    /// come; [`Renderer::apply_due`] then applies it like a launch or a stop. A column with a
    /// launch or stop queued by hand skips its clip's follow action.
    fn follow_up(&mut self) {
        let Self { columns, random, beat, .. } = self;
        for column in columns.iter_mut() {
            let Some(playing) = column.playing.as_mut().filter(|p| !p.followed) else { continue };
            let Some(follow) = column.slots.get(playing.slot).and_then(|s| s.clip.as_ref()).and_then(|c| c.follow)
            else {
                continue;
            };
            if !follow.due(playing.elapsed_beats) {
                continue;
            }
            playing.followed = true;
            if column.pending.is_some() {
                continue;
            }
            let kind = follow.pick(random);
            let slots = &column.slots;
            let playable = |slot: usize| slots.get(slot).is_some_and(|s| s.clip.is_some());
            let action = match crate::follow::resolve(kind, playing.slot, slots.len(), playable, random) {
                Outcome::Keep => continue,
                Outcome::Stop => Queued::Stop,
                Outcome::Launch(slot) => Queued::Launch(slot),
            };
            column.pending = Some(Pending { at_beat: *beat, action, repeat: false });
        }
    }

    fn publish(&self) {
        let m = &self.monitor;
        m.beat.store(self.beat.to_bits(), Ordering::Relaxed);
        m.tempo.store(self.tempo_bpm.to_bits(), Ordering::Relaxed);
        self.publish_meter();
        for (i, column) in self.columns.iter().enumerate() {
            let playing = column.playing.map_or(NONE, |p| u32::try_from(p.slot).unwrap_or(NONE));
            let queued = match column.pending.map(|p| p.action) {
                None => NONE,
                Some(Queued::Stop) => STOP,
                Some(Queued::Launch(slot)) => u32::try_from(slot).unwrap_or(NONE).min(STOP - 1),
            };
            if let Some(a) = m.playing.get(i) {
                a.store(playing, Ordering::Relaxed);
            }
            if let Some(a) = m.queued.get(i) {
                a.store(queued, Ordering::Relaxed);
            }
            if let Some(a) = m.overrides.get(i) {
                a.store(column.overrides_since.is_some(), Ordering::Relaxed);
            }
        }
        m.columns.store(self.columns.len().min(m.playing.len()), Ordering::Release);
    }
}

/// A playing or fading clip carried from its slot in `old_slots` to the slot `map` gives in
/// `new_slots`, or `None` if that slot is gone or has no clip now. Its stretch voices taken over
/// from the old graph play on; the others go to where the clip is.
fn carry(
    new_slots: &mut [SlotPlan],
    old_slots: &[SlotPlan],
    state: Option<Playing>,
    map: impl Fn(usize) -> Option<usize>,
    frames_per_beat: f64,
    voices: &prepare::Voices,
) -> Option<Playing> {
    let mut playing = state?;
    let old_slot = playing.slot;
    playing.slot = map(old_slot)?;
    let new_plan = new_slots.get_mut(playing.slot)?.clip.as_mut()?;
    let old_plan = old_slots.get(old_slot).and_then(|s| s.clip.as_ref());
    // Notes: the same place in the new list. Notes at exactly the last beat may have played.
    let last = playing.last_clip_beat;
    let played_at_last = old_plan.map_or(0, |plan| playing.next_note.saturating_sub(plan.first_note_from(last)));
    let at_last = new_plan.notes.partition_point(|n| n.time <= last) - new_plan.first_note_from(last);
    playing.next_note = new_plan.first_note_from(last) + played_at_last.min(at_last);
    for a in new_plan.audio.iter_mut().filter(|a| a.voice.is_none_or(|i| voices.claim(i) != prepare::TAKEN)) {
        if let Some(voice) = a.stretch.as_mut() {
            if voices.inline() && playing.stretched {
                voice.prepare();
            }
            prepare::to_playing(voice, &playing, frames_per_beat);
        }
    }
    Some(playing)
}

/// A time signature in one atomic word, for the monitor.
fn pack(ts: TimeSignature) -> u32 {
    u32::from(ts.numerator) << 8 | u32::from(ts.denominator)
}

/// Beats in one bar: a 4/4 bar has 4, a 6/8 bar 3.
fn bar_beats(ts: TimeSignature) -> f64 {
    if ts.numerator == 0 || ts.denominator == 0 {
        return 4.0;
    }
    f64::from(ts.numerator) * 4.0 / f64::from(ts.denominator)
}

fn plan_clip(
    project: &Project,
    audio: &AudioPool,
    chains: &[Chain],
    clip: &suhina_model::Clip,
    track: suhina_model::TrackId,
    frames_per_beat: f64,
    sample_rate: u32,
) -> Option<ClipPlan> {
    let content = project.content(clip.content)?;
    if !(clip.length.is_finite() && clip.length > 0.0 && clip.offset.is_finite()) {
        return None;
    }
    let loop_range = clip
        .loop_range
        .filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start)
        .map(|r| (r.start, r.end));
    let mut notes = Vec::new();
    let mut audio_plans = Vec::new();
    for lane in &content.lanes {
        let lane_track = lane.target.unwrap_or(track);
        let Some((bus, (gain_left, gain_right))) = route(project, chains, lane_track) else {
            continue;
        };
        let meter = crate::meters::meter_slot(project, lane_track);
        match &lane.data {
            LaneData::Notes(lane_notes) => {
                for note in lane_notes.iter().filter(|n| !n.muted && n.time.is_finite() && n.duration.is_finite()) {
                    let velocity = note_velocity(note.velocity);
                    notes.push(PlanNote {
                        time: note.time,
                        duration: note.duration.max(0.0),
                        frequency: 440.0 * 2f64.powf((f64::from(note.key) - 69.0) / 12.0),
                        key: note.key.min(127),
                        velocity,
                        track: lane_track,
                        gain_left: gain_left * velocity,
                        gain_right: gain_right * velocity,
                        bus,
                        meter,
                        scale: crate::musical_scale(content.scale),
                    });
                }
            }
            LaneData::Audio(region) => {
                let Some(data) = audio.files.get(region.pool_key().as_ref()) else { continue };
                // Placed at frame 0; a looping clip is given an endless length. The fades are
                // fitted to the clip's own length: the fade-in plays once from the launch, the
                // fade-out where the clip ends by itself, which a looping clip never does
                // (`docs/spec/clip-launching.md` › Clip fades).
                let (fade_in, fade_out) = clip.fitted_fades();
                let placed = suhina_model::Clip {
                    start: 0.0,
                    length: if loop_range.is_some() { ENDLESS_BEATS } else { clip.length },
                    fade_in,
                    fade_out: if loop_range.is_some() { 0.0 } else { fade_out },
                    ..clip.clone()
                };
                let gains = (gain_left * AUDIO_HEADROOM, gain_right * AUDIO_HEADROOM);
                let tempo = project.transport.tempo_bpm;
                // Launched clips are not on the arrangement's timeline: they are placed at the
                // start tempo and follow the launcher clock.
                let timing = Timing { frames_per_beat, tempo_bpm: tempo, sample_rate, map: None };
                let Some(event) = AudioEvent::new(&placed, region, data.clone(), timing, gains) else { continue };
                // Warped clips are time-stretched like arrangement clips; the stretch voice gets
                // the region gain, the direct reader has it already.
                let stretch_beats = if loop_range.is_some() { STRETCH_LOOP_BEATS } else { clip.length };
                let stretch = (region.warped && region.stretch != suhina_model::StretchMode::Repitch)
                    .then(|| {
                        let stretched = suhina_model::Clip { length: stretch_beats, ..placed.clone() };
                        StretchVoice::new(&stretched, region, data.clone(), tempo, sample_rate)
                    })
                    // Prepared later, off the audio thread (`prepare`).
                    .flatten();
                let region_gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
                let fingerprint = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::hash::DefaultHasher::new();
                    format!("{region:?}|{placed:?}|{stretch_beats}|{tempo}|{sample_rate}").hash(&mut hasher);
                    // The decoded audio itself: new samples under the same file are another voice.
                    (Arc::as_ptr(data) as usize).hash(&mut hasher);
                    hasher.finish()
                };
                audio_plans.push(PlanAudio {
                    event,
                    follows_tempo: region.warped,
                    stretch,
                    voice: None,
                    stretch_beats,
                    stretch_gains: (gains.0 * region_gain, gains.1 * region_gain),
                    fingerprint,
                    bus,
                    meter,
                });
            }
        }
    }
    notes.sort_by(|a, b| a.time.total_cmp(&b.time));
    Some(ClipPlan {
        offset: crate::audio::start_beat(clip.offset, loop_range),
        length: clip.length,
        loop_range,
        notes,
        audio: audio_plans,
        follow: FollowPlan::new(clip),
    })
}

impl Renderer {
    /// A controller for sending launch commands from another thread. The renderer applies them
    /// at the start of each [`Renderer::process`] call. Calling this again replaces the queue.
    pub fn controller(&mut self) -> Controller {
        let (producer, consumer) = rtrb::RingBuffer::new(QUEUE_CAPACITY);
        self.graph.launcher.control = Some(consumer);
        Controller {
            producer,
            monitor: Arc::clone(&self.graph.launcher.monitor),
            updater: self.updater(),
            preview: self.preview.sender(),
            preview_monitor: Arc::clone(&self.preview.monitor),
            cpu_monitor: Arc::clone(&self.cpu.monitor),
            meter_monitor: Arc::clone(&self.meters.monitor),
            live: Some(self.live.sender()),
            recorder: Some(self.recorder.receiver()),
        }
    }

    /// The launcher state, as other threads see it.
    pub fn monitor(&self) -> Arc<LauncherMonitor> {
        Arc::clone(&self.graph.launcher.monitor)
    }

    /// The audio thread's CPU load, as other threads see it.
    pub fn cpu_monitor(&self) -> Arc<crate::cpu::CpuMonitor> {
        Arc::clone(&self.cpu.monitor)
    }

    /// The level meters, as other threads see them.
    pub fn meter_monitor(&self) -> Arc<crate::meters::MeterMonitor> {
        Arc::clone(&self.meters.monitor)
    }

    /// Applies one launch command now. Never allocates.
    pub fn handle(&mut self, command: LaunchCommand) {
        match command {
            LaunchCommand::Play => {
                if !self.running && self.start_waiting.is_none() {
                    self.start_when_ready();
                }
            }
            // A stop during the count-in only ends the count: the transport never started, so
            // the play start marker stays where it is.
            LaunchCommand::Stop | LaunchCommand::StopHere if self.count_in.is_some() => self.cancel_count_in(),
            // A stop before the waiting start rolled: it never did.
            LaunchCommand::Stop | LaunchCommand::StopHere if self.start_waiting.is_some() => self.start_waiting = None,
            LaunchCommand::Stop => self.stop_transport(false),
            LaunchCommand::StopHere => self.stop_transport(true),
            LaunchCommand::SetRecording { on } => self.recorder.set(on),
            LaunchCommand::CountIn { beats, bar_beats } => self.start_count_in(beats, bar_beats),
            LaunchCommand::SetPosition { beat } => self.set_position(beat),
            LaunchCommand::Locate { beat } => {
                if self.running && beat.is_finite() {
                    self.jump_to(beat.clamp(0.0, MAX_START_BEAT));
                    self.chase = true;
                }
            }
            LaunchCommand::ResumeColumn { column, slot, elapsed_beats } => {
                self.resume_column(column, slot, elapsed_beats)
            }
            LaunchCommand::SetLaunchTempo { bpm } => {
                if bpm.is_finite() && (1.0..=999.0).contains(&bpm) {
                    self.graph.launcher.leave_map();
                    self.graph.launcher.tempo_bpm = bpm;
                    self.graph.launcher.pending_tempo = None;
                }
            }
            LaunchCommand::SetArmed { track, armed, input } => {
                let frame = self.block_frame;
                self.live.set_armed(&mut self.graph.chains, track, armed, input, frame);
            }
            LaunchCommand::SetMidiInput { track, armed, monitoring, input } => {
                let frame = self.block_frame;
                self.live.set_input(&mut self.graph.chains, track, armed, monitoring, input, frame);
            }
            LaunchCommand::SetMetronome { on, gain } => {
                self.metronome.set(on, gain);
                self.graph.launcher.monitor.metronome.store(self.metronome.on(), Ordering::Relaxed);
            }
            LaunchCommand::SetMetronomeSound(sound) => self.metronome.set_sound(sound),
            LaunchCommand::SetCue { gain, separate } => self.cue.set(gain, separate),
            LaunchCommand::FollowTempoMap => {
                self.graph.launcher.follow_map();
                self.clock_to_map();
            }
            LaunchCommand::LaunchClip { column, slot } => {
                let Some(target) = self.graph.launcher.columns.get(column).and_then(|c| c.slots.get(slot)) else {
                    return;
                };
                let action = match (&target.clip, target.has_clip, target.has_stop) {
                    (Some(_), _, _) => Queued::Launch(slot),
                    (None, false, true) => Queued::Stop,
                    (None, _, _) => return,
                };
                if matches!(action, Queued::Stop) && !self.running {
                    return;
                }
                self.ensure_running();
                match action {
                    Queued::Launch(slot) => self.graph.launcher.press(column, slot),
                    Queued::Stop => self.graph.launcher.queue(column, action),
                }
            }
            LaunchCommand::ReleaseClip { column, slot } => {
                if self.running {
                    self.graph.launcher.release(column, slot);
                } else if let Some(c) = self.graph.launcher.columns.get_mut(column) {
                    c.held = None;
                }
            }
            LaunchCommand::LaunchScene { scene } => {
                if scene >= self.graph.launcher.scene_tempos.len() {
                    return;
                }
                self.ensure_running();
                for column in 0..self.graph.launcher.columns.len() {
                    let action = match self.graph.launcher.columns.get(column).and_then(|c| c.slots.get(scene)) {
                        Some(SlotPlan { clip: Some(_), .. }) => Queued::Launch(scene),
                        Some(SlotPlan { clip: None, has_clip: false, has_stop: true, .. }) => Queued::Stop,
                        _ => continue,
                    };
                    self.graph.launcher.queue(column, action);
                }
                let launcher = &mut self.graph.launcher;
                if let Some(Some(bpm)) = launcher.scene_tempos.get(scene) {
                    launcher.pending_tempo = Some((launcher.launch_beat(), *bpm));
                }
                if let Some(Some(signature)) = launcher.scene_signatures.get(scene) {
                    launcher.pending_signature = Some((launcher.launch_beat(), *signature));
                }
            }
            LaunchCommand::StopColumn { column } => {
                if self.running {
                    self.graph.launcher.queue(column, Queued::Stop);
                }
            }
            LaunchCommand::StopScene { scene } => {
                if self.running {
                    for column in 0..self.graph.launcher.columns.len() {
                        // Only the tracks whose slot in the row has a clip: an empty slot has
                        // nothing to stop, and its track may be playing from somewhere else.
                        if let Some(SlotPlan { clip: Some(_), .. }) =
                            self.graph.launcher.columns.get(column).and_then(|c| c.slots.get(scene))
                        {
                            self.graph.launcher.queue(column, Queued::Stop);
                        }
                    }
                }
            }
            LaunchCommand::StopAll => {
                if self.running {
                    for column in 0..self.graph.launcher.columns.len() {
                        self.graph.launcher.queue(column, Queued::Stop);
                    }
                }
            }
            LaunchCommand::SetQuantization(q) => self.graph.launcher.quantization = q,
            LaunchCommand::SeedRandom(seed) => self.graph.launcher.random.seed(seed),
            LaunchCommand::BackToArrangement => {
                // Every track that plays launched clips stops them (fading out, as a stop does)
                // and plays its arrangement again.
                for index in 0..self.graph.launcher.columns.len() {
                    let launcher = &mut self.graph.launcher;
                    let Some(column) = launcher.columns.get_mut(index) else { continue };
                    if column.overrides_since.take().is_none() {
                        continue;
                    }
                    column.pending = None;
                    if let Some(mut playing) = column.playing.take() {
                        playing.fade_left = launcher.stop_fade_frames;
                        column.fading = Some(playing);
                    }
                    self.release_column(index);
                }
            }
        }
        self.graph.launcher.publish();
    }

    fn ensure_running(&mut self) {
        if !self.running {
            self.start_transport();
        }
    }

    /// Starts the transport once the stretched clips at the marker are rendered ahead (at once
    /// when they are, offline, or without any), so their beginnings are not lost to a worker
    /// still filling their rings.
    fn start_when_ready(&mut self) {
        if !self.stretch_fresh {
            self.graph.ahead.move_to(self.start_frame());
            self.stretch_fresh = true;
        }
        if self.graph.ahead.ready_at(self.start_frame()) {
            self.start_transport();
        } else {
            self.start_waiting = Some(0);
        }
    }

    /// Moves a waiting start along before a block of `frames`: rolls when the stretched clips
    /// are ready or it has waited [`crate::START_WAIT_SECONDS`].
    pub(crate) fn wait_to_start(&mut self, frames: u64) {
        let Some(waited) = self.start_waiting else { return };
        self.graph.ahead.service();
        let limit = (crate::START_WAIT_SECONDS * f64::from(self.sample_rate)) as u64;
        if waited >= limit || self.graph.ahead.ready_at(self.start_frame()) {
            self.start_waiting = None;
            self.start_transport();
        } else {
            self.start_waiting = Some(waited + frames);
        }
    }

    pub(crate) fn start_transport(&mut self) {
        self.start_waiting = None;
        self.running = true;
        self.count_in = None;
        // A start plays the arrangement's tempo and bars again; a scene launched with it takes
        // the clock off the map again at its launch beat.
        self.graph.launcher.follow_map();
        // The stretch voices are moved to the marker when the transport stops or the marker
        // moves, so the worker has them ready by the next start; only a start without either
        // before it moves them here.
        if !self.stretch_fresh {
            self.graph.ahead.move_to(self.start_frame());
            self.stretch_fresh = true;
        }
        self.locate(self.start_beat);
        self.chase = true;
        self.graph.launcher.monitor.running.store(true, Ordering::Relaxed);
    }

    /// Plays the arrangement from `beat` from the next frame on: notes that start there or
    /// later, audio clips from wherever they are at that beat. A start or a jump also chases the
    /// notes that started before it and still sound (`Renderer::chase_notes`); a loop's wrap does
    /// not (`docs/spec/arrangement-editing.md` › Play position).
    fn locate(&mut self, beat: f64) {
        self.origin = self.position;
        self.time_at_origin = self.frame_of_beat(beat);
        let time = self.time_at_origin;
        self.next_event = self.graph.events.partition_point(|e| e.start < time);
        // Audio clips that started before `time` and are still playing there start mid-clip.
        self.next_audio = 0;
        self.active_audio_count = 0;
        self.metronome.reset_at(beat);
        self.graph.launcher.beat = beat;
        self.graph.launcher.tempo_from_map();
    }

    /// The arrangement frame of `beat`, through the playing graph's tempo map.
    pub(crate) fn frame_of_beat(&self, beat: f64) -> u64 {
        crate::tempo::frame_of(&self.graph.tempo, beat)
    }

    /// The play start marker's arrangement frame.
    pub(crate) fn start_frame(&self) -> u64 {
        self.frame_of_beat(self.start_beat)
    }

    /// The arrangement beat playing now.
    fn playhead_beat(&self) -> f64 {
        crate::tempo::beat_at(&self.graph.tempo, self.time())
    }

    /// Ends every sounding note and device tail that the transport stopping or jumping cuts.
    fn release_sounding(&mut self) {
        for voice in self.voices.iter_mut().filter(|v| v.active) {
            voice.end = voice.end.min(self.position);
        }
        let frame = self.block_frame;
        for chain in &mut self.graph.chains {
            chain.release_all(frame);
        }
        self.active_audio_count = 0;
    }

    /// Stops the transport; the playhead goes back to the marker, or, with `here`, the marker
    /// moves to the playhead. Stopping while stopped moves the marker to beat 0.
    fn stop_transport(&mut self, here: bool) {
        self.start_beat = if !self.running {
            0.0
        } else if here {
            self.playhead_beat().clamp(0.0, MAX_START_BEAT)
        } else {
            self.start_beat
        };
        self.running = false;
        self.metronome.reset();
        self.release_sounding();
        // Ready for the next start, at the marker.
        self.graph.ahead.move_to(self.start_frame());
        self.stretch_fresh = true;
        let launcher = &mut self.graph.launcher;
        for column in &mut launcher.columns {
            column.pending = None;
            if let Some(mut playing) = column.playing.take() {
                playing.fade_left = launcher.stop_fade_frames;
                column.fading = Some(playing);
            }
        }
        launcher.pending_tempo = None;
        launcher.pending_signature = None;
        // A scene's time signature stays while stopped, its bars counted from the start again;
        // the next start puts the clock back on the tempo map.
        launcher.bar_origin = 0.0;
        launcher.bars_before = 0.0;
        launcher.beat = self.start_beat;
        launcher.tempo_from_map();
        launcher.monitor.running.store(false, Ordering::Relaxed);
    }

    /// Moves the play start marker to `beat`; while running, playback jumps there. Launches and
    /// a scene tempo still waiting are queued again for the next boundary from the new beat.
    fn set_position(&mut self, beat: f64) {
        if !beat.is_finite() {
            return;
        }
        self.start_beat = beat.clamp(0.0, MAX_START_BEAT);
        if !self.running {
            self.graph.ahead.move_to(self.start_frame());
            self.stretch_fresh = true;
            self.graph.launcher.beat = self.start_beat;
            self.graph.launcher.tempo_from_map();
            return;
        }
        self.jump_to(self.start_beat);
        // A click on the ruler is a jump like `Locate`: the notes sounding there play.
        self.chase = true;
    }

    /// The loop region as arrangement frames, while playback repeats it: the region is on and
    /// at least one frame long.
    fn loop_frames(&self) -> Option<(u64, u64)> {
        if !self.follow_loop {
            return None;
        }
        let (start, end) = self.graph.loop_beats?;
        let (start, end) = (self.frame_of_beat(start), self.frame_of_beat(end));
        (end > start).then_some((start, end))
    }

    /// Frames until the playhead reaches the loop end, while it is running before it. A
    /// playhead past the loop end (moved there) plays on without wrapping.
    pub(crate) fn frames_to_loop_end(&self) -> Option<usize> {
        if !self.running {
            return None;
        }
        let (_, end) = self.loop_frames()?;
        let to_end = end.checked_sub(self.time()).filter(|frames| *frames > 0)?;
        Some(usize::try_from(to_end).unwrap_or(usize::MAX))
    }

    /// At the loop end, playback jumps back to the loop start, as a jump while playing does:
    /// notes sounding are released, notes and audio clips play on from the loop start
    /// (`docs/spec/arrangement-editing.md` › Loop region). The play start marker stays.
    pub(crate) fn wrap_loop(&mut self) {
        if !self.running {
            return;
        }
        let Some((_, end)) = self.loop_frames() else { return };
        if self.time() != end {
            return;
        }
        if let Some((start, _)) = self.graph.loop_beats {
            self.jump_to(start);
        }
        self.graph.launcher.publish();
    }

    /// Brings the launcher clock, back on the tempo map, to the arrangement: while running its
    /// beat becomes the playhead's (a scene tempo had let it drift), launched clips play on from
    /// where they are, and launches still waiting are queued again for the next boundary from
    /// there, as after a jump. Never allocates.
    pub(crate) fn clock_to_map(&mut self) {
        if !self.graph.launcher.follows_map {
            return;
        }
        if self.running {
            let beat = self.playhead_beat();
            self.metronome.reset_at(beat);
            let launcher = &mut self.graph.launcher;
            launcher.beat = beat;
            let at_beat = launcher.launch_beat();
            for column in &mut launcher.columns {
                if let Some(pending) = column.pending.as_mut() {
                    pending.at_beat = at_beat;
                }
            }
        }
        self.graph.launcher.tempo_from_map();
        self.graph.launcher.publish();
    }

    /// While running, playback continues from `beat` from the next frame on.
    fn jump_to(&mut self, beat: f64) {
        self.release_sounding();
        self.locate(beat);
        // Read from the new place; until the worker gets there, stretched clips are silent.
        self.graph.ahead.move_to(self.time_at_origin);
        self.stretch_fresh = false;
        let launcher = &mut self.graph.launcher;
        let at_beat = launcher.launch_beat();
        for column in &mut launcher.columns {
            if let Some(pending) = column.pending.as_mut() {
                pending.at_beat = at_beat;
            }
        }
        if let Some((at, _)) = launcher.pending_tempo.as_mut() {
            *at = at_beat;
        }
        if let Some((at, _)) = launcher.pending_signature.as_mut() {
            *at = at_beat;
        }
    }

    /// [`LaunchCommand::ResumeColumn`]: the column plays `slot`'s clip as if launched
    /// `elapsed_beats` ago, or nothing, and its track no longer plays its arrangement.
    fn resume_column(&mut self, column: usize, slot: Option<usize>, elapsed_beats: f64) {
        if !self.running || column >= self.graph.launcher.columns.len() {
            return;
        }
        let elapsed = if elapsed_beats.is_finite() { elapsed_beats.clamp(0.0, ENDLESS_BEATS) } else { 0.0 };
        if let Some(slot) = slot {
            self.graph.launcher.prepare_inline(column, slot);
        }
        let launcher = &mut self.graph.launcher;
        let (tempo, project_tempo) = (launcher.tempo_bpm, launcher.project_tempo_bpm);
        let frames_per_beat = launcher.frames_per_beat_at_build;
        let frames_per_beat_now = 60.0 / tempo * f64::from(self.sample_rate);
        let Some(target) = launcher.columns.get_mut(column) else { return };
        let playing = match slot {
            None => None,
            Some(slot) => {
                let Some(plan) = target.slots.get_mut(slot).and_then(|s| s.clip.as_mut()) else { return };
                let stretched = tempo == project_tempo;
                if stretched {
                    let frames = (elapsed * frames_per_beat).round().max(0.0) as u64;
                    for voice in plan.audio.iter_mut().filter_map(|a| a.stretch.as_mut()) {
                        voice.seek(frames);
                    }
                }
                let beat = plan.clip_beat(elapsed);
                Some(Playing {
                    slot,
                    elapsed_beats: elapsed,
                    elapsed_frames: elapsed * frames_per_beat_now,
                    last_clip_beat: beat,
                    next_note: plan.first_note_from(beat),
                    fade_left: 0.0,
                    stretched,
                    // A follow action whose time had come ran on the old output already; one
                    // still ahead runs here when its time comes.
                    followed: plan.follow.is_some_and(|f| f.due(elapsed)),
                })
            }
        };
        target.pending = None;
        target.fading = None;
        target.playing = playing;
        target.overrides_since = Some(self.position);
        self.release_column(column);
    }

    /// Applies commands queued by a [`Controller`]. Called at the start of each block.
    pub(crate) fn drain_commands(&mut self) {
        let mut handled = 0;
        while handled < QUEUE_CAPACITY {
            let Some(command) = self.graph.launcher.control.as_mut().and_then(|c| c.pop().ok()) else { break };
            self.handle(command);
            handled += 1;
        }
        // After the commands, so a launch they queued asks for its voices at once.
        self.graph.launcher.service_voices();
        let launcher = &self.graph.launcher;
        let monitor = &launcher.monitor;
        monitor.beat.store(launcher.beat.to_bits(), Ordering::Relaxed);
        monitor.launch_beat.store(launcher.launch_beat().to_bits(), Ordering::Relaxed);
        monitor.tempo.store(launcher.tempo_bpm.to_bits(), Ordering::Relaxed);
        for (column, elapsed) in launcher.columns.iter().zip(&monitor.elapsed) {
            if let Some(playing) = column.playing {
                elapsed.store(playing.elapsed_beats.to_bits(), Ordering::Relaxed);
            }
        }
        self.graph.launcher.publish_meter();
    }

    /// Advances the launcher by one frame: applies launches that are due, starts notes, mixes
    /// audio and fades. Returns the launcher's stereo output for this frame.
    pub(crate) fn launcher_frame(&mut self) -> (f32, f32) {
        let mut out = (0.0f32, 0.0f32);
        let frame = self.block_frame;
        if self.running {
            self.graph.launcher.follow_up();
            self.apply_due();
        }
        // The beats this frame moves on. On a changing tempo map the next beat is read from the
        // map at the next arrangement frame, so the clock never drifts from the arrangement;
        // otherwise the tempo is added up frame by frame, as it always was.
        let beats_per_frame = self.graph.launcher.tempo_bpm / 60.0 / f64::from(self.sample_rate);
        let on_map = self.running && self.graph.launcher.on_map();
        let (step, next_beat) = if on_map {
            let next = crate::tempo::beat_at(&self.graph.tempo, self.time().saturating_add(1));
            let next = next.max(self.graph.launcher.beat);
            (next - self.graph.launcher.beat, next)
        } else {
            (beats_per_frame, self.graph.launcher.beat + beats_per_frame)
        };
        for index in 0..self.graph.launcher.columns.len() {
            if self.running {
                self.play_notes(index, beats_per_frame, on_map);
            }
            let launcher = &mut self.graph.launcher;
            let chains = &mut self.graph.chains;
            let levels = &mut self.graph.levels;
            let fpb = launcher.frames_per_beat_at_build;
            let fade_total = launcher.stop_fade_frames;
            let (tempo, project_tempo) = (launcher.tempo_bpm, launcher.project_tempo_bpm);
            let Some(column) = launcher.columns.get_mut(index) else { continue };
            for state in [&mut column.playing, &mut column.fading] {
                let Some(p) = state.as_mut() else { continue };
                let Some(plan) = column.slots.get_mut(p.slot).and_then(|s| s.clip.as_mut()) else {
                    *state = None;
                    continue;
                };
                let gain = if p.fade_left > 0.0 { (p.fade_left / fade_total) as f32 } else { 1.0 };
                if p.stretched && tempo != project_tempo {
                    p.stretched = false;
                }
                for a in &mut plan.audio {
                    let (l, r) = match a.stretch.as_mut() {
                        Some(voice) if p.stretched && p.elapsed_beats < a.stretch_beats => {
                            let mut frame = [0.0f32; 2];
                            voice.render(&mut frame);
                            // The same fades as the direct reader: in at the start, out at a
                            // clip end.
                            let until_end = a.event.end as f64 - p.elapsed_frames;
                            let fade = a.event.fades().gain(p.elapsed_frames, until_end);
                            (frame[0] * a.stretch_gains.0 * fade, frame[1] * a.stretch_gains.1 * fade)
                        }
                        _ => {
                            let at = if a.follows_tempo { p.elapsed_beats * fpb } else { p.elapsed_frames };
                            a.event.sample_elapsed(at)
                        }
                    };
                    match bus_chain(chains, a.bus) {
                        Some(chain) => chain.add(frame, l * gain, r * gain),
                        None => {
                            levels.add(a.meter, frame, l * gain, r * gain);
                            out.0 += l * gain;
                            out.1 += r * gain;
                        }
                    }
                }
                p.elapsed_beats += step;
                p.elapsed_frames += 1.0;
                if p.fade_left > 0.0 {
                    p.fade_left -= 1.0;
                    if p.fade_left <= 0.0 {
                        *state = None;
                    }
                }
            }
            // A clip without a loop stops at its end marker.
            if let Some(p) = column.playing
                && let Some(plan) = column.slots.get(p.slot).and_then(|s| s.clip.as_ref())
                && plan.loop_range.is_none()
                && p.elapsed_beats >= plan.length
                && (p.followed || plan.follow.is_none())
            {
                column.playing = None;
                self.graph.launcher.publish();
            }
        }
        if self.running {
            self.graph.launcher.beat = next_beat;
            if on_map {
                self.graph.launcher.tempo_from_map();
            }
        }
        out
    }

    /// Applies queued launches, stops and tempo changes whose beat has come.
    fn apply_due(&mut self) {
        let beat = self.graph.launcher.beat;
        if let Some((at, bpm)) = self.graph.launcher.pending_tempo
            && at <= beat
        {
            self.graph.launcher.leave_map();
            self.graph.launcher.tempo_bpm = bpm;
            self.graph.launcher.pending_tempo = None;
        }
        let launcher = &mut self.graph.launcher;
        if let Some((at, signature)) = launcher.pending_signature
            && at <= beat
        {
            launcher.leave_map();
            let bars = ((at - launcher.bar_origin) / launcher.bar_beats - 1e-9).ceil().max(0.0);
            launcher.bars_before += bars;
            launcher.time_signature = signature;
            launcher.bar_beats = bar_beats(signature);
            launcher.bar_origin = at;
            launcher.pending_signature = None;
        }
        let mut changed = false;
        for index in 0..self.graph.launcher.columns.len() {
            let Some(column) = self.graph.launcher.columns.get_mut(index) else { continue };
            let Some(pending) = column.pending.filter(|p| p.at_beat <= beat) else { continue };
            column.pending = None;
            changed = true;
            let previous = column.playing.map(|p| (p.elapsed_beats, p.elapsed_frames));
            if let Some(mut old) = column.playing.take() {
                old.fade_left = self.graph.launcher.stop_fade_frames;
                column.fading = Some(old);
            }
            if let Queued::Launch(slot) = pending.action {
                // Offline the clip's voices are built here; otherwise they have arrived, or join
                // in where the clip is once they do.
                self.graph.launcher.prepare_inline(index, slot);
            }
            let Some(column) = self.graph.launcher.columns.get_mut(index) else { continue };
            if let Queued::Launch(slot) = pending.action
                && let Some(target) = column.slots.get_mut(slot)
                && let Some(plan) = target.clip.as_mut()
            {
                // A stretch voice can play one launch at a time: a relaunch of the clip that
                // is fading out cuts its fade short.
                if column.fading.is_some_and(|f| f.slot == slot) {
                    column.fading = None;
                }
                let mut playing = Playing {
                    slot,
                    elapsed_beats: 0.0,
                    elapsed_frames: 0.0,
                    last_clip_beat: plan.offset,
                    next_note: plan.first_note_from(plan.offset),
                    fade_left: 0.0,
                    stretched: self.graph.launcher.tempo_bpm == self.graph.launcher.project_tempo_bpm,
                    followed: false,
                };
                let legato = target.launch.legato;
                // A legato clip takes over the position of the clip it replaces.
                let frames_per_beat = self.graph.launcher.frames_per_beat_at_build;
                if let (true, Some(previous)) = (legato, previous) {
                    modes::take_over(plan, &mut playing, previous, frames_per_beat);
                } else {
                    for a in &mut plan.audio {
                        if let Some(voice) = a.stretch.as_mut() {
                            voice.restart();
                        }
                    }
                }
                column.playing = Some(playing);
                column.overrides_since = Some(self.position);
                self.graph.launcher.launched(index, slot, pending.at_beat);
            }
            self.release_column(index);
        }
        if changed {
            self.graph.launcher.publish();
        }
    }

    /// Releases the notes a column is sounding, launched or from the arrangement.
    fn release_column(&mut self, column: usize) {
        let Ok(column) = u32::try_from(column) else { return };
        let position = self.position;
        for voice in self.voices.iter_mut().filter(|v| v.active && v.column == column) {
            voice.end = voice.end.min(position);
        }
        let frame = self.block_frame;
        for chain in self.graph.chains.iter_mut().filter(|c| c.column == column) {
            chain.release_all(frame);
        }
        // Arrangement audio on this track goes silent through the override check.
    }

    /// Starts the notes of a column's playing clip that are due at this frame.
    fn play_notes(&mut self, index: usize, beats_per_frame: f64, on_map: bool) {
        let time = self.time();
        let Self { graph, voices, position, sample_rate, block_frame, .. } = self;
        let map = &graph.tempo;
        let frame = *block_frame;
        let launcher = &mut graph.launcher;
        let chains = &mut graph.chains;
        let Ok(column_id) = u32::try_from(index) else { return };
        let Some(column) = launcher.columns.get_mut(index) else { return };
        let Some(p) = column.playing.as_mut() else { return };
        let Some(plan) = column.slots.get(p.slot).and_then(|s| s.clip.as_ref()) else { return };
        let limit = plan.note_limit();
        let beat = plan.clip_beat(p.elapsed_beats);
        let song_beat = launcher.beat;
        let mut start = |note: &PlanNote| {
            let beats = note.duration.min(limit - note.time).max(0.0);
            // A note's length in frames follows the tempo map from where it starts.
            let frames = if on_map {
                crate::tempo::frame_of(map, song_beat + beats).saturating_sub(time)
            } else {
                (beats / beats_per_frame).round() as u64
            };
            let end = position.saturating_add(frames);
            let start = NoteStart {
                frequency: note.frequency,
                key: note.key,
                velocity: note.velocity,
                track: note.track,
                gains: (note.gain_left, note.gain_right),
                column: column_id,
                bus: note.bus,
                meter: note.meter,
                scale: note.scale,
            };
            start_note(voices, chains, frame, *position, *sample_rate, end, start);
        };
        if let Some((loop_start, loop_end)) = plan.loop_range
            && beat < p.last_clip_beat
        {
            // Wrapped: finish the notes before the loop end, then start the loop again.
            while let Some(note) = plan.notes.get(p.next_note).filter(|n| n.time < loop_end) {
                start(note);
                p.next_note += 1;
            }
            p.next_note = plan.first_note_from(loop_start);
        }
        while let Some(note) = plan.notes.get(p.next_note).filter(|n| n.time <= beat && n.time < limit) {
            start(note);
            p.next_note += 1;
        }
        p.last_clip_beat = beat;
    }
}

#[cfg(test)]
#[path = "launcher_tests.rs"]
mod tests;
