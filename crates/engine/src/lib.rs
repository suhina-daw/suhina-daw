// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina audio engine. Offline rendering and real-time playback share this code path:
//! [`Renderer::new`] does all allocation up front, and [`Renderer::process`] never allocates,
//! so the same `process` call can later run on the audio thread.

mod ahead;
pub use assets::DeviceAssets;
mod assets;
mod audio;
mod automation;
#[cfg(test)]
mod automation_render_tests;
mod buses;
#[cfg(test)]
mod buses_tests;
mod chain;
#[cfg(test)]
mod chain_tests;
mod count_in;
mod cpu;
mod cue;
#[cfg(test)]
mod cue_tests;
#[cfg(test)]
mod fade_cost_tests;
#[cfg(test)]
mod fade_tests;
mod follow;
#[cfg(test)]
mod follow_tests;
mod group;
#[cfg(test)]
mod group_tests;
mod input;
#[cfg(test)]
mod input_tests;
mod launcher;
mod live;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod loop_tests;
mod meters;
#[cfg(test)]
mod meters_tests;
mod metronome;
#[cfg(test)]
mod midi_effect_tests;
mod plugin;
mod plugin_params;
#[cfg(test)]
mod plugin_params_tests;
#[cfg(test)]
mod plugin_tests;
mod preview;
#[cfg(test)]
mod preview_tests;
mod record;
#[cfg(test)]
mod record_tests;
mod sends;
#[cfg(test)]
mod sends_tests;
mod sine;
mod stretch;
mod swap;
mod tempo;
#[cfg(test)]
mod tempo_tests;
mod track_render;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ahead::AheadReader;
pub use ahead::StretchWorker;
pub use audio::{AudioData, AudioPool};
use audio::{AudioEvent, Fades};
use chain::{CHAIN_INPUT_GAIN, Chain, MAX_BLOCK, run_as_answered};
pub use cpu::CpuMonitor;
pub use cue::MAX_CUE_GAIN;
pub use group::{GROUP_PARAMS, SILENT_DB, group_param_index, group_param_values};
pub use input::MAX_INPUT_CHANNELS;
use launcher::Launcher;
pub use launcher::{
    ColumnStatus, Controller, LaunchCommand, LauncherMonitor, MAX_START_BEAT, PlayingMeter, Quantization, Queued,
    SceneShift,
};
pub use live::{
    LIVE_QUEUE, LiveEvent, LiveFilter, LivePorts, LiveSender, LiveSource, MAX_ARMED, MAX_SOURCES, StampClock, live_now,
    port_id,
};
pub use meters::{Level, MAX_METERED_TRACKS, MeterMonitor, PEAK_FALL_DB_PER_SECOND, RMS_SECONDS};
pub use metronome::ClickSound;
use plugin::{DEADLINE_SHARE, Delay};
pub use plugin::{
    Followed, Joint, MAX_JOINED, OFFLINE_TIMEOUT, PluginDevice, PluginProcessor, PluginSlotInfo, Plugins, Sent,
    plugin_devices, plugin_key,
};
pub use plugin_params::{MAX_BLOCK_PARAMS, ParamEvent, RAMP_STEP, param_id};
pub use preview::{PreviewMonitor, PreviewNote, PreviewNotes, preview_notes};
pub use record::{RECORD_CHUNK, RECORD_CHUNKS, RECORD_NOTES, RecordReceiver, RecordedAudio, RecordedNote};
use stretch::StretchVoice;
pub use suhina_device_api::NoteEvent as DeviceEvent;
use suhina_device_api::{MusicalScale, ProcessContext};
pub use suhina_model as model;
use suhina_model::{ChannelId, ChannelRole, Clip, ContentLane, LaneData, Project, TempoMap, TrackId};
pub use track_render::render_track;

/// The most frames a plug-in is asked to process at once: the size of a device chain's block.
pub const MAX_BLOCK_FRAMES: usize = MAX_BLOCK;
/// Voices that can sound at once. A note that arrives when all are busy steals the oldest.
const MAX_VOICES: usize = 64;
/// Audio clips that can play at once; more overlapping clips than this are skipped.
const MAX_AUDIO_VOICES: usize = 64;
/// Audio clips play at unity gain (0 dB) on a centred track: this undoes the headroom scaling
/// that `track_gains` applies for the sine voices.
const AUDIO_HEADROOM: f32 = 4.0;
/// Voice and event tag for "not on a launcher column".
const NO_COLUMN: u32 = u32::MAX;
/// Project frames rendered per stretch-voice call.
const STRETCH_BLOCK: usize = 1024;
/// The device index of a parameter message that moves a track's volume fader
/// ([`Controller::set_device_param`]): the value is the channel's new linear volume, and the
/// playing graph takes it without a new graph where the track's chain can.
pub const MIXER_VOLUME: usize = usize::MAX;

/// What the last graph's build took, part by part, for the trace (`docs/debugging.md`): an
/// edit to a large project held the window up for over a second there.
pub mod build_report {
    use std::sync::Mutex;

    static LAST: Mutex<String> = Mutex::new(String::new());

    pub(crate) fn set(report: String) {
        if let Ok(mut last) = LAST.lock() {
            *last = report;
        }
    }

    pub(crate) fn add(part: &str) {
        if let Ok(mut last) = LAST.lock() {
            last.push_str(", ");
            last.push_str(part);
        }
    }

    /// The parts of the last graph built and sent.
    pub fn last() -> String {
        LAST.lock().map(|l| l.clone()).unwrap_or_default()
    }
}
/// How long a start waits at most for the stretched clips at the marker to be rendered ahead,
/// in seconds; it rolls then whatever they have.
const START_WAIT_SECONDS: f64 = 0.3;
/// Release time after note-off, in seconds.
const RELEASE_SECONDS: f64 = 0.05;
/// Attack time, in seconds; long enough to avoid a click at note-on.
const ATTACK_SECONDS: f64 = 0.002;
/// Upper bound for an offline render, so a malformed project cannot exhaust memory.
const MAX_RENDER_SECONDS: f64 = 60.0 * 60.0;
/// Extra length of a render when tracks have devices, for instrument releases and effect tails.
const CHAIN_TAIL_SECONDS: f64 = 2.0;
/// Most delay latency compensation makes up for, in seconds.
pub(crate) const MAX_LATENCY_SECONDS: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    InvalidProject(Vec<suhina_model::ValidationError>),
    InvalidSampleRate(u32),
    InvalidTempo(f64),
    TooLong { seconds: f64 },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProject(errors) => {
                write!(f, "invalid project:")?;
                for e in errors {
                    write!(f, " {e};")?;
                }
                Ok(())
            }
            Self::InvalidSampleRate(sr) => write!(f, "sample rate {sr} is not supported"),
            Self::InvalidTempo(bpm) => write!(f, "tempo {bpm} bpm is not supported"),
            Self::TooLong { seconds } => {
                write!(f, "render would be {seconds:.0} s long; the limit is {MAX_RENDER_SECONDS:.0} s")
            }
        }
    }
}

impl std::error::Error for RenderError {}

/// A warped clip placement; its stretcher is the voice with the same index in the renderer's
/// [`AheadReader`]. `start` and `end` are arrangement frames.
struct StretchEvent {
    start: u64,
    end: u64,
    gain_left: f32,
    gain_right: f32,
    /// The click guard and the clip's fades, as the direct reader has them.
    fades: Fades,
    /// The launcher column of the clip's track, or `NO_COLUMN`.
    column: u32,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
    /// The track's meter slot (`meters::meter_slot`).
    meter: u32,
}

/// A note with its position resolved to sample frames and its mix resolved from the channel.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NoteEvent {
    start: u64,
    end: u64,
    frequency: f64,
    key: u8,
    velocity: f32,
    track: TrackId,
    gain_left: f32,
    gain_right: f32,
    /// The launcher column of the note's track, or `NO_COLUMN`.
    column: u32,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
    /// The track's meter slot.
    meter: u32,
    /// The scale of the note's clip, for the MIDI effects that follow it.
    scale: Option<MusicalScale>,
}

/// A clip's scale as the devices get it.
pub(crate) fn musical_scale(scale: Option<suhina_model::Scale>) -> Option<MusicalScale> {
    scale.and_then(|scale| MusicalScale::new(scale.root(), scale.mode.semitones()))
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Voice {
    active: bool,
    started_at: u64,
    end: u64,
    phase: f64,
    phase_step: f64,
    gain_left: f32,
    gain_right: f32,
    /// The id of the track it plays on and its velocity, so an edit to the track's mix
    /// reaches it.
    track: u32,
    velocity: f32,
    /// The launcher column whose track this voice plays on, or `NO_COLUMN`.
    column: u32,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
    /// The track's meter slot.
    meter: u32,
}

/// An arrangement audio clip and the launcher column of its track.
#[derive(Debug, Clone)]
struct ArrangedAudio {
    event: AudioEvent,
    column: u32,
    bus: u32,
    meter: u32,
    /// Which clip of the project this is (arrangement lane, clip, content lane), so a graph swap
    /// can find the same clip in the old graph (ADR 0005).
    key: (usize, usize, usize),
}

/// Plays a project: its arrangement from the start, and launcher clips as they are launched
/// (see [`Renderer::handle`]). Build it once, then call [`Renderer::process`] block by block.
/// Everything a renderer derives from the project. An edit builds a new one off the audio thread
/// and the renderer swaps it in (ADR 0005).
pub(crate) struct Graph {
    /// The tracks that hear the audio input, and whether each is heard this block.
    inputs: Vec<input::InputRoute>,
    inputs_heard: Vec<bool>,
    events: Vec<NoteEvent>,
    audio_events: Vec<ArrangedAudio>,
    /// Warped clips stretched as they play (ADR 0003), rendered ahead and mixed block by block.
    stretch_events: Vec<StretchEvent>,
    ahead: AheadReader,
    length: u64,
    launcher: Launcher,
    /// The tempo map counted in frames: every beat of this graph is placed through it
    /// (`docs/spec/tempo-map.md`).
    tempo: Arc<TempoMap>,
    /// The span playback repeats, in beats (`Transport::looping`): the transport turns them
    /// into frames through the tempo map as it plays, so the loop keeps its beats when the
    /// tempo changes.
    loop_beats: Option<(f64, f64)>,
    /// Device chains of the tracks that have devices (ADR 0004).
    chains: Vec<Chain>,
    /// Every track's bus and the gains its sources are mixed at (muted tracks at zero), so
    /// sounding voices follow an edit to their track's mix.
    routes: Vec<(TrackId, u32, (f32, f32))>,
    /// Latency compensation: the most delay any chain adds, in frames, and the delay of
    /// everything that does not go through a chain (each chain delays itself up to it).
    latency: u32,
    direct_delay: Delay,
    /// Every track's block of sound, for its level meter.
    levels: meters::Levels,
}

pub struct Renderer {
    sample_rate: u32,
    graph: Box<Graph>,
    next_event: usize,
    /// The transport started or jumped: the notes that span the playhead start on the next
    /// frame, as if their beginning were there (`docs/spec/arrangement-editing.md` › Play
    /// position).
    chase: bool,
    voices: [Voice; MAX_VOICES],
    next_audio: usize,
    /// Indices into the graph's `audio_events` that are playing; the first `active_audio_count`
    /// are valid.
    active_audio: [usize; MAX_AUDIO_VOICES],
    active_audio_count: usize,
    /// The stretch voices are at their first frame and nothing has played them since.
    stretch_fresh: bool,
    /// A start waiting for the stretched clips at the marker to be rendered, with the frames
    /// waited so far: the transport rolls once they are, so their beginnings are heard (a
    /// locate waits for the read-ahead in OSS hosts too), or after [`START_WAIT_SECONDS`].
    start_waiting: Option<u64>,
    stretch_scratch: Vec<f32>,
    /// Frames rendered since the renderer was built.
    position: u64,
    /// The frame at which the transport last started or the tempo last changed, and the
    /// arrangement frame playing then: arrangement time is `position - origin + time_at_origin`.
    origin: u64,
    time_at_origin: u64,
    /// The play start marker, in beats: where the transport starts and, while stopped, the
    /// playhead (`docs/spec/arrangement-editing.md` › Play position).
    start_beat: f64,
    running: bool,
    /// What the graph was built from, so a controller can build the next one (ADR 0005).
    audio: Arc<AudioPool>,
    /// The file-backed devices' files, for the next graph (docs/spec/nam-amp-and-cab.md).
    assets: Arc<DeviceAssets>,
    swap: swap::SwapState,
    /// Stretch underruns counted by graphs swapped out.
    underruns_before: u64,
    metronome: metronome::Metronome,
    /// The frame of the current block being rendered, for events sent to device chains.
    block_frame: usize,
    /// Rendering offline: plug-ins are waited for without a real-time deadline (ADR 0006).
    offline: bool,
    /// What is known about the project's plug-in devices, for the graphs built after edits.
    plugin_info: HashMap<u64, PluginSlotInfo>,
    /// The browser's preview voice (ADR 0011).
    preview: preview::Preview,
    /// The cue bus: cued tracks and the preview, for headphones (`cue.rs`).
    cue: cue::Cue,
    /// Notes played on a MIDI controller, and the armed tracks that hear them (ADR 0017).
    live: live::Live,
    /// The audio input device's frames, for the tracks that hear it (ADR 0017).
    input: input::Input,
    /// What the armed tracks play, handed out while recording (`docs/spec/recording.md`).
    recorder: record::Recorder,
    /// The count-in before recording starts the transport, while it counts.
    count_in: Option<count_in::CountIn>,
    /// The audio thread's CPU load, published to the controller's `CpuMonitor`.
    cpu: cpu::CpuMeter,
    /// The level meters, published to the controller's `MeterMonitor`.
    meters: meters::MeterWriter,
    /// Playback repeats the project's loop region; off for a render of the whole arrangement.
    follow_loop: bool,
}

impl Renderer {
    /// A renderer for the project's notes; audio clips stay silent. See [`Renderer::with_audio`].
    pub fn new(project: &Project, sample_rate: u32) -> Result<Self, RenderError> {
        Self::with_audio(project, &AudioPool::new(), sample_rate)
    }

    /// A renderer that also plays audio clips whose files are in `audio`.
    pub fn with_audio(project: &Project, audio: &AudioPool, sample_rate: u32) -> Result<Self, RenderError> {
        Self::with_assets(project, audio, &DeviceAssets::new(), sample_rate)
    }

    /// A renderer whose file-backed devices run the parsed models and decoded IRs `assets`
    /// holds, by each device's stored path ([docs/spec/nam-amp-and-cab.md]).
    pub fn with_assets(
        project: &Project,
        audio: &AudioPool,
        assets: &DeviceAssets,
        sample_rate: u32,
    ) -> Result<Self, RenderError> {
        Self::with_assets_plugins(project, audio, assets, sample_rate, Plugins::new())
    }

    /// A renderer whose plug-in devices run in the processors `plugins` holds (ADR 0006).
    pub fn with_plugins(
        project: &Project,
        audio: &AudioPool,
        sample_rate: u32,
        plugins: Plugins,
    ) -> Result<Self, RenderError> {
        Self::with_assets_plugins(project, audio, &DeviceAssets::new(), sample_rate, plugins)
    }

    /// A renderer whose plug-in devices run in the processors `plugins` holds, and whose
    /// file-backed devices run the parsed models and decoded IRs `assets` holds, by each
    /// device's stored path ([docs/spec/nam-amp-and-cab.md]).
    pub fn with_assets_plugins(
        project: &Project,
        audio: &AudioPool,
        assets: &DeviceAssets,
        sample_rate: u32,
        mut plugins: Plugins,
    ) -> Result<Self, RenderError> {
        let graph = Graph::build(project, audio, assets, sample_rate, &mut plugins)?;
        graph.launcher.monitor.running.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut meters = meters::MeterWriter::new(sample_rate);
        meters.assign(graph.levels.tracks());
        graph
            .launcher
            .monitor
            .frames_per_beat
            .store(graph.tempo.start_per_beat().to_bits(), std::sync::atomic::Ordering::Relaxed);
        Ok(Self {
            sample_rate,
            graph,
            next_event: 0,
            chase: false,
            voices: [Voice::default(); MAX_VOICES],
            next_audio: 0,
            active_audio: [0; MAX_AUDIO_VOICES],
            active_audio_count: 0,
            stretch_fresh: true,
            start_waiting: None,
            stretch_scratch: vec![0.0; STRETCH_BLOCK * 2],
            position: 0,
            origin: 0,
            time_at_origin: 0,
            start_beat: 0.0,
            running: true,
            audio: Arc::new(audio.clone()),
            assets: Arc::new(assets.clone()),
            swap: swap::SwapState::default(),
            underruns_before: 0,
            metronome: metronome::Metronome::new(sample_rate),
            block_frame: 0,
            offline: false,
            plugin_info: plugins.info,
            preview: preview::Preview::new(sample_rate),
            cue: cue::Cue::new(),
            live: live::Live::new(sample_rate),
            input: input::Input::new(),
            recorder: record::Recorder::new(),
            count_in: None,
            cpu: cpu::CpuMeter::new(),
            meters,
            follow_loop: true,
        })
    }

    /// A renderer whose transport is stopped: silent until it receives [`LaunchCommand::Play`]
    /// or a launch.
    pub fn stopped(project: &Project, audio: &AudioPool, sample_rate: u32) -> Result<Self, RenderError> {
        Self::stopped_with_plugins(project, audio, sample_rate, Plugins::new())
    }

    /// [`Renderer::stopped`] with plug-in devices running in `plugins`.
    pub fn stopped_with_plugins(
        project: &Project,
        audio: &AudioPool,
        sample_rate: u32,
        plugins: Plugins,
    ) -> Result<Self, RenderError> {
        let mut renderer = Self::with_plugins(project, audio, sample_rate, plugins)?;
        renderer.running = false;
        renderer.graph.launcher.monitor.running.store(false, std::sync::atomic::Ordering::Relaxed);
        Ok(renderer)
    }

    /// [`Renderer::with_assets`], with the transport stopped.
    pub fn stopped_with_assets(
        project: &Project,
        audio: &AudioPool,
        assets: &DeviceAssets,
        sample_rate: u32,
        plugins: Plugins,
    ) -> Result<Self, RenderError> {
        let mut renderer = Self::with_assets_plugins(project, audio, assets, sample_rate, plugins)?;
        renderer.running = false;
        renderer.graph.launcher.monitor.running.store(false, std::sync::atomic::Ordering::Relaxed);
        Ok(renderer)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Renders offline from now on: plug-ins are waited for as long as they take (up to
    /// [`OFFLINE_TIMEOUT`] a block) instead of until the block's real-time deadline.
    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    /// Whether playback repeats the project's loop region when it is on (the default). A render
    /// of the whole arrangement turns this off, so it plays from start to end once.
    pub fn set_follow_loop(&mut self, follow: bool) {
        self.follow_loop = follow;
    }

    /// Ends the renderer and gives back its plug-in devices' processors, with what is known
    /// about each, so another renderer (a new audio output at the same sample rate) can be built
    /// with them ([`Renderer::stopped_with_plugins`]) and the plug-ins keep running. Call it
    /// once the renderer no longer runs on the audio thread: it frees what the graph holds.
    pub fn into_plugins(mut self) -> Plugins {
        let mut taken = swap::Taken::default();
        self.swap.take_processors(&mut self.graph, &mut taken);
        // What the slots knew is newer than what the renderer was built with: the controller
        // tells the graphs after it, as plug-ins load and say their latency.
        let mut info = std::mem::take(&mut self.plugin_info);
        info.extend(taken.info);
        let processors: HashMap<u64, Box<dyn PluginProcessor>> = taken.processors;
        for key in processors.keys() {
            info.entry(*key).or_default();
        }
        Plugins { info, processors }
    }

    /// The delay latency compensation adds to everything, in frames: the most any track's
    /// devices add.
    pub fn latency(&self) -> u32 {
        self.graph.latency
    }

    /// Length of the arrangement including release tails, in frames.
    pub fn length(&self) -> u64 {
        self.graph.length
    }

    /// The arrangement frame playing now; while stopped, the play start marker's.
    fn time(&self) -> u64 {
        if self.running { self.position - self.origin + self.time_at_origin } else { self.start_frame() }
    }

    /// Whether the transport is running.
    pub fn running(&self) -> bool {
        self.running
    }

    /// Takes the worker that renders stretched clips ahead, to run it on its own thread
    /// ([`StretchWorker::spawn`]) so that the audio thread only copies their frames. Without
    /// this the renderer stretches them itself as it goes, as offline rendering does. `None` once
    /// taken.
    pub fn take_stretch_worker(&mut self) -> Option<StretchWorker> {
        self.graph.ahead.take_worker()
    }

    /// Frames of stretched clips that were not ready in time and played as silence.
    /// Reads the audio input device's frames from `ring`, `channels` (1 to
    /// [`MAX_INPUT_CHANNELS`]) interleaved per frame, from now on; the tracks with an audio input
    /// hear them as their monitoring says (ADR 0017). Blocks that find too little input count in
    /// `underruns`. Call it before the renderer moves to the audio thread.
    pub fn set_audio_input(
        &mut self,
        ring: rtrb::Consumer<f32>,
        channels: usize,
        underruns: Arc<std::sync::atomic::AtomicU64>,
    ) {
        self.input.attach(ring, channels, underruns);
    }

    /// Whether a track is armed to play the live input (ADR 0017): someone may be playing, so
    /// the output keeps less queued ahead of the device. Lock-free; for the render thread.
    pub fn live_armed(&self) -> bool {
        self.live.any_armed()
    }

    /// Where the render thread is on the live clock ([`live_now`]): it renders the next block
    /// at `now`. Notes stamped with a time are played `delay` frames after it, at the frame
    /// where that falls (#269): a delay as long as the render thread's longest wait between two
    /// blocks and one block more leaves every note a frame in the block after it, so notes keep
    /// the spacing they were played with. Call it before every [`Renderer::process`]; without
    /// it, every note plays at the first frame of the next block. Never allocates.
    pub fn set_live_clock(&mut self, now: u64, delay: u32) {
        self.live.set_clock(now, delay, self.position);
    }

    /// The output's latency (from the engine to the speaker, measured) and the input device's,
    /// in frames: a recorded note or sound is placed at the beat that was heard when it was
    /// played (`docs/spec/recording.md` › Lined up with what was heard). Call it before every
    /// [`Renderer::process`] while a device plays; offline both are 0. Never allocates.
    pub fn set_record_latency(&mut self, output: u32, input: u32) {
        self.recorder.set_latency(output, input);
    }

    /// The renderer frame each live note was played at, in order.
    #[cfg(test)]
    pub(crate) fn live_played(&self) -> Vec<u64> {
        self.live.played.iter().map(|(at, _)| *at).collect()
    }

    pub fn stretch_underruns(&self) -> u64 {
        self.underruns_before + self.graph.ahead.underruns()
    }

    /// Publishes the plug-in blocks missed so far and the plug-in that missed most. Never
    /// allocates.
    fn publish_plugin_misses(&mut self) {
        let (mut total, mut worst) = (0u64, (0u64, 0u64));
        for chain in &mut self.graph.chains {
            for slot in chain.plugin_slots() {
                total += slot.missed;
                if slot.missed > worst.1 {
                    worst = (slot.key, slot.missed);
                }
            }
        }
        let monitor = &self.graph.launcher.monitor;
        monitor.plugin_misses.store(total, std::sync::atomic::Ordering::Relaxed);
        monitor.worst_plugin.store(worst.0, std::sync::atomic::Ordering::Relaxed);
        monitor.worst_misses.store(worst.1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Starts the notes that began before arrangement frame `time` and still sound there, at
    /// frame `frame` of the block, each ending where it would have: a start or a jump inside a
    /// note plays it (from its attack, as an instrument cannot start a sound part-way). Never
    /// allocates.
    fn chase_notes(&mut self, frame: usize, time: u64) {
        for index in 0..self.next_event.min(self.graph.events.len()) {
            let Some(event) = self.graph.events.get(index).copied() else { break };
            if event.end <= time || self.graph.launcher.overrides(event.column) {
                continue;
            }
            let end = self.position + (event.end - time);
            let start = NoteStart {
                frequency: event.frequency,
                key: event.key,
                velocity: event.velocity,
                track: event.track,
                gains: (event.gain_left, event.gain_right),
                column: event.column,
                bus: event.bus,
                meter: event.meter,
                scale: event.scale,
            };
            let Self { voices, graph, position, sample_rate, .. } = self;
            start_note(voices, &mut graph.chains, frame, *position, *sample_rate, end, start);
        }
    }

    /// Renders the next `out.len() / 2` frames of interleaved stereo into `out`, overwriting it.
    /// Commands from a [`Controller`] are applied first. Never allocates. The cue bus is
    /// rendered too but not given out; see [`Renderer::process_with_cue`].
    pub fn process(&mut self, out: &mut [f32]) {
        self.process_parts(out, None);
    }

    /// Like [`Renderer::process`], and writes the cue bus, the same number of frames of
    /// interleaved stereo, into `cue` (overwriting it; frames `cue` has no room for are not
    /// written, and frames past `out`'s are left alone). Never allocates.
    pub fn process_with_cue(&mut self, out: &mut [f32], cue: &mut [f32]) {
        self.process_parts(out, Some(cue));
    }

    fn process_parts(&mut self, out: &mut [f32], cue: Option<&mut [f32]>) {
        // The CPU load is measured only in real time: an offline render runs flat out.
        let measured = if self.offline { None } else { Some(Instant::now()) };
        self.receive_swap();
        self.receive_plugins();
        let mut rest = out;
        let mut cue_rest = cue;
        // Every new graph whose frame falls in this block takes over at its frame: the frames
        // before it are the previous graph's. The queue is bounded, so this ends.
        loop {
            let frames = (rest.len() / 2) as u64;
            match self.swap_at() {
                Some(at) if at < self.position + frames => {
                    let split = ((at.saturating_sub(self.position) * 2) as usize).min(rest.len());
                    let (before, after) = rest.split_at_mut(split);
                    let (cue_before, cue_after) = split_cue(cue_rest, split);
                    if !before.is_empty() {
                        self.process_block(before, cue_before);
                    }
                    self.apply_swap();
                    rest = after;
                    cue_rest = cue_after;
                }
                _ => break,
            }
        }
        if !rest.is_empty() {
            self.process_block(rest, cue_rest);
        }
        self.publish_position();
        self.publish_plugin_misses();
        self.publish_count_in();
        if let Some(started) = measured {
            self.cpu.measured(started, Instant::now());
        }
    }

    fn process_block(&mut self, mut out: &mut [f32], mut cue: Option<&mut [f32]>) {
        // Device chains hold at most MAX_BLOCK frames; longer blocks are rendered in pieces. A
        // piece also ends at the loop end, where the transport wraps to the loop start, so the
        // wrap lands on its exact frame. Commands are applied first: they can start the
        // transport or move it, which changes where the loop end falls.
        while !out.is_empty() {
            self.wrap_loop();
            self.drain_commands();
            // A count-in that has counted its beats starts the transport on this frame.
            self.end_count_in();
            let frames = (out.len() / 2).min(MAX_BLOCK);
            self.wait_to_start(frames as u64);
            let frames = self.frames_to_loop_end().map_or(frames, |to_end| frames.min(to_end.max(1)));
            let frames = self.frames_to_count_in_end().map_or(frames, |to_end| frames.min(to_end.max(1)));
            let (chunk, rest) = std::mem::take(&mut out).split_at_mut(frames * 2);
            let (cue_chunk, cue_after) = split_cue(cue, chunk.len());
            self.process_chunk(chunk, cue_chunk);
            cue = cue_after;
            out = rest;
        }
    }

    /// When the plug-ins of a block of `frames` frames starting now must have answered: a share
    /// of the block's playing time, so a late plug-in costs its own sound, not the device's
    /// deadline. `None` offline.
    fn deadline(&self, frames: usize) -> Option<Instant> {
        if self.offline {
            return None;
        }
        let seconds = frames as f64 / f64::from(self.sample_rate) * DEADLINE_SHARE;
        Some(Instant::now() + Duration::from_secs_f64(seconds))
    }

    fn process_chunk(&mut self, out: &mut [f32], cue: Option<&mut [f32]>) {
        let frames = out.len() / 2;
        self.cue.begin(frames);
        let deadline = if self.graph.chains.is_empty() { None } else { self.deadline(frames) };
        for chain in &mut self.graph.chains {
            chain.begin(frames, deadline);
        }
        self.block_frame = 0;
        // Preview commands apply once the chains have begun the block, so a stopped preview's
        // note-offs land on this block's first frame, not the previous block's length later
        // (#156); the playing preview finds its chain first (the graph may have been swapped),
        // and a new one after.
        self.preview.begin(&self.graph.chains);
        self.preview.receive(&mut self.graph.chains);
        self.preview.begin(&self.graph.chains);
        // This block's audio input, and which tracks hear it: In always, Auto while armed.
        self.input.read(frames);
        // Where this block is, for what is recorded in it: the beats of its first frame.
        let beats_per_frame = self.graph.launcher.tempo_bpm() / 60.0 / f64::from(self.sample_rate);
        let arrangement_beat = crate::tempo::beat_at(&self.graph.tempo, self.time());
        self.recorder.begin(
            self.running,
            self.position,
            self.graph.launcher.beat(),
            arrangement_beat,
            beats_per_frame,
            self.live.delay(),
            self.input.waiting(),
        );
        // Live notes after the commands, so a track armed in this block hears them.
        self.live.receive(&mut self.graph.chains, &mut self.recorder, self.position);
        for (route, heard) in self.graph.inputs.iter().zip(self.graph.inputs_heard.iter_mut()) {
            *heard = match route.monitor {
                suhina_model::Monitor::In => true,
                suhina_model::Monitor::Auto => self.live.is_armed(route.track),
                suhina_model::Monitor::Off => false,
            };
        }
        // An armed track's input is recorded as it comes in, whatever its monitoring.
        if self.recorder.takes_audio() {
            for route in self.graph.inputs.iter().filter(|r| self.live.is_armed(r.track)) {
                let channels = if route.left == route.right { 1 } else { 2 };
                let (left, right) = (route.left, route.right);
                let input = &self.input;
                self.recorder.audio(route.track, channels, 0, frames, |frame, channel| {
                    input.sample(frame, if channel == 0 { left } else { right })
                });
            }
        }
        self.receive_params();
        // A rewind moves along even while the transport is stopped, so the stretched clips are
        // ready again by the next start.
        self.graph.ahead.service();
        let context = ProcessContext {
            tempo: self.graph.launcher.tempo_bpm(),
            beat: self.graph.launcher.beat(),
            playing: self.running,
        };
        // Plug-in parameter events (set values, automation) are planned before any of the
        // block runs: a chain may run part of it early, when notes crowd it.
        let at = self.running.then(|| self.time());
        let time = self.time();
        for chain in &mut self.graph.chains {
            chain.transport(self.running, time, context.tempo);
            chain.automate(at, frames);
            chain.automate_mix(time, frames, self.running);
        }
        let attack = (ATTACK_SECONDS * f64::from(self.sample_rate)).max(1.0);
        let release = (RELEASE_SECONDS * f64::from(self.sample_rate)).max(1.0);
        let block_start = self.position;
        // The transport changes only through commands, which are applied above.
        let running = self.running;
        for (frame, [out_left, out_right]) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            self.block_frame = frame;
            for chain in self.graph.chains.iter_mut().filter(|c| c.instrument) {
                chain.release_due(frame, self.position);
            }
            self.preview.frame(&mut self.graph.chains, frame, self.position);
            self.live.frame(&mut self.graph.chains, &mut self.recorder, frame, self.position);
            // The click for this frame's beat, read before the launcher moves the beat on.
            let launcher = &self.graph.launcher;
            let click = self.metronome.frame(self.running, launcher.beat(), |index| launcher.starts_bar(index))
                + self.count_in_frame();
            let (launched_left, launched_right) = self.launcher_frame();
            let time = self.time();
            if self.running && self.chase {
                self.chase = false;
                self.chase_notes(frame, time);
            }
            while self.running
                && let Some(event) = self.graph.events.get(self.next_event)
            {
                if event.start > time {
                    break;
                }
                let event = *event;
                self.next_event += 1;
                if !self.graph.launcher.overrides(event.column) {
                    let end = self.position + event.end.saturating_sub(event.start);
                    let start = NoteStart {
                        frequency: event.frequency,
                        key: event.key,
                        velocity: event.velocity,
                        track: event.track,
                        gains: (event.gain_left, event.gain_right),
                        column: event.column,
                        bus: event.bus,
                        meter: event.meter,
                        scale: event.scale,
                    };
                    let Self { voices, graph, position, sample_rate, .. } = self;
                    start_note(voices, &mut graph.chains, frame, *position, *sample_rate, end, start);
                }
            }

            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for voice in self.voices.iter_mut().filter(|v| v.active) {
                let age = (self.position - voice.started_at) as f64;
                let envelope = if self.position < voice.end {
                    (age / attack).min(1.0)
                } else {
                    let released = (self.position - voice.end) as f64;
                    if released >= release {
                        voice.active = false;
                        continue;
                    }
                    ((voice.end - voice.started_at) as f64 / attack).min(1.0) * (1.0 - released / release)
                };
                let sample = (voice.phase * std::f64::consts::TAU).sin() * envelope;
                voice.phase = (voice.phase + voice.phase_step).fract();
                let (l, r) = (sample as f32 * voice.gain_left, sample as f32 * voice.gain_right);
                match bus_chain(&mut self.graph.chains, voice.bus) {
                    Some(chain) => chain.add(frame, l, r),
                    None => {
                        self.graph.levels.add(voice.meter, frame, l, r);
                        left += l;
                        right += r;
                    }
                }
            }
            for (route, heard) in self.graph.inputs.iter().zip(&self.graph.inputs_heard) {
                if !heard {
                    continue;
                }
                let l = self.input.sample(frame, route.left) * route.gains.0;
                let r = self.input.sample(frame, route.right) * route.gains.1;
                match bus_chain(&mut self.graph.chains, route.bus) {
                    Some(chain) => chain.add(frame, l, r),
                    None => {
                        left += l;
                        right += r;
                    }
                }
            }
            let (audio_left, audio_right) = if self.running { self.audio_frame(time, frame) } else { (0.0, 0.0) };
            *out_left = left + audio_left + launched_left + click;
            *out_right = right + audio_right + launched_right + click;
            self.position += 1;
        }
        if running {
            self.mix_stretched(out, block_start);
        }
        // What went straight to the output waits for the slowest chain (latency compensation).
        self.graph.direct_delay.run(out);
        // The chains no chain plays into run their plug-ins at the same time, so the block's
        // deadline is shared by the slowest chain rather than by all of them in turn: each
        // chain runs until it hands a block to a plug-in, and goes on as soon as that plug-in
        // answers, while the others are still out (#151). A bus runs after the chains that play
        // into it, in order below.
        let chains = &mut self.graph.chains;
        for chain in chains.iter_mut().filter(|c| !c.is_bus) {
            chain.advance(frames, &context);
        }
        run_as_answered(chains, frames, &context);
        // In order, so a bus or a return has every chain that plays into it before it runs.
        sends::finish(&mut self.graph.chains, out, &mut self.cue.bus, frames, &context);
        self.graph.levels.finish(&self.graph.chains, frames, &mut self.meters);
        // The preview comes after every track and the main channel's mix (ADR 0011), into the
        // cue bus, and into the main output too while the cue plays with it.
        if let Some(preview) = self.cue.preview.get_mut(..frames * 2) {
            self.preview.finish(preview, frames, &mut self.graph.chains);
        }
        self.cue.finish(out, frames);
        if let Some(cue) = cue
            && let Some(bus) = self.cue.bus.get(..frames * 2)
        {
            for (to, from) in cue.iter_mut().zip(bus) {
                *to = *from;
            }
        }
        self.meters.measure(meters::Meter::Main, out, frames);
        self.block_frame = 0;
    }

    /// Adds the stretched clips that overlap the block starting at frame `block_start` to `out`.
    /// Clips on a track that plays launched clips keep running, so they stay in time, but are
    /// not heard.
    fn mix_stretched(&mut self, out: &mut [f32], block_start: u64) {
        // Arrangement time of the block's first frame.
        let time_start = block_start - self.origin + self.time_at_origin;
        let time_end = time_start + (out.len() / 2) as u64;
        let scratch = &mut self.stretch_scratch;
        let launcher = &self.graph.launcher;
        let ahead = &mut self.graph.ahead;
        // Which voices the worker renders first: the ones due from here.
        ahead.set_playhead(time_start);
        let chains = &mut self.graph.chains;
        let levels = &mut self.graph.levels;
        self.stretch_fresh = false;
        for (index, event) in self.graph.stretch_events.iter().enumerate() {
            if event.end <= time_start || event.start >= time_end {
                continue;
            }
            let mut time = event.start.max(time_start);
            let to = event.end.min(time_end);
            while time < to {
                let frames = ((to - time) as usize).min(STRETCH_BLOCK);
                let Some(chunk) = scratch.get_mut(..frames * 2) else { return };
                ahead.read(index, time - event.start, chunk);
                for (k, [left, right]) in chunk.as_chunks::<2>().0.iter().enumerate() {
                    let frame = time + k as u64;
                    if launcher.overrides_at(event.column, frame - time_start + block_start) {
                        continue;
                    }
                    let since_start = (frame - event.start) as f64;
                    let until_end = (event.end - frame) as f64;
                    let fade = event.fades.gain(since_start, until_end);
                    let at = (frame - time_start) as usize;
                    let (l, r) = (left * event.gain_left * fade, right * event.gain_right * fade);
                    if let Some(chain) = bus_chain(chains, event.bus) {
                        chain.add(at, l, r);
                    } else if let Some([ol, or]) =
                        out.get_mut(at * 2..at * 2 + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok())
                    {
                        levels.add(event.meter, at, l, r);
                        *ol += l;
                        *or += r;
                    }
                }
                time += frames as u64;
            }
        }
    }

    /// Starts arrangement audio clips that begin at arrangement frame `time`, mixes the playing
    /// ones and drops the ones that have ended. Clips on a track with devices go to its chain at
    /// `frame` of the block; the rest are returned.
    fn audio_frame(&mut self, time: u64, frame: usize) -> (f32, f32) {
        while let Some(arranged) = self.graph.audio_events.get(self.next_audio) {
            if arranged.event.start > time {
                break;
            }
            if arranged.event.end > time
                && let Some(slot) = self.active_audio.get_mut(self.active_audio_count)
            {
                *slot = self.next_audio;
                self.active_audio_count += 1;
            }
            self.next_audio += 1;
        }
        let (mut left, mut right) = (0.0f32, 0.0f32);
        let mut i = 0;
        while i < self.active_audio_count {
            let index = self.active_audio.get(i).copied().unwrap_or(usize::MAX);
            match self.graph.audio_events.get(index) {
                Some(arranged) if time < arranged.event.end => {
                    if !self.graph.launcher.overrides(arranged.column) {
                        let (l, r) = arranged.event.sample(time);
                        match bus_chain(&mut self.graph.chains, arranged.bus) {
                            Some(chain) => chain.add(frame, l, r),
                            None => {
                                self.graph.levels.add(arranged.meter, frame, l, r);
                                left += l;
                                right += r;
                            }
                        }
                    }
                    i += 1;
                }
                _ => {
                    // Ended: move the last active entry into this slot.
                    self.active_audio_count -= 1;
                    let last = self.active_audio.get(self.active_audio_count).copied().unwrap_or(usize::MAX);
                    if let Some(slot) = self.active_audio.get_mut(i) {
                        *slot = last;
                    }
                }
            }
        }
        (left, right)
    }
}

/// A note to start: its sine voice's pitch and gains, or the key and velocity an instrument
/// gets.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NoteStart {
    pub(crate) frequency: f64,
    pub(crate) key: u8,
    pub(crate) velocity: f32,
    pub(crate) track: TrackId,
    /// The track's gains times the velocity.
    pub(crate) gains: (f32, f32),
    pub(crate) column: u32,
    pub(crate) bus: u32,
    pub(crate) meter: u32,
    /// The scale of the note's clip.
    pub(crate) scale: Option<MusicalScale>,
}

/// The chain behind `bus`, or `None` for the output.
pub(crate) fn bus_chain(chains: &mut [Chain], bus: u32) -> Option<&mut Chain> {
    let index = usize::try_from(bus).ok()?.checked_sub(1)?;
    chains.get_mut(index)
}

/// Starts a note at renderer frame `position` (`frame` of the block) that ends at frame `end`:
/// on the track's instrument when its chain starts with one, otherwise as a sine voice, stealing
/// the oldest voice when all are busy.
pub(crate) fn start_note(
    voices: &mut [Voice; MAX_VOICES],
    chains: &mut [Chain],
    frame: usize,
    position: u64,
    sample_rate: u32,
    end: u64,
    note: NoteStart,
) {
    if let Some(chain) = bus_chain(chains, note.bus).filter(|c| c.instrument) {
        chain.set_scale(frame, note.scale);
        chain.note_on(frame, position, note.key, note.velocity, end);
        return;
    }
    let slot = match voices.iter().position(|v| !v.active) {
        Some(free) => free,
        None => voices.iter().enumerate().min_by_key(|(_, v)| v.started_at).map_or(0, |(i, _)| i),
    };
    if let Some(voice) = voices.get_mut(slot) {
        *voice = Voice {
            active: true,
            started_at: position,
            end: end.max(position + 1),
            phase: 0.0,
            phase_step: note.frequency / f64::from(sample_rate),
            gain_left: note.gains.0,
            gain_right: note.gains.1,
            track: note.track.0,
            velocity: note.velocity,
            column: note.column,
            bus: note.bus,
            meter: note.meter,
        };
    }
}

/// Where a track's sound goes: its bus (0 for the output, 1 + the chain's index when the track
/// has a chain) and the gains its sources are mixed at; `None` when the track is silent and has no
/// chain. A track with a chain is mixed at a centred unity gain, and its own gains apply after
/// the chain.
pub(crate) fn route(project: &Project, chains: &[Chain], track: TrackId) -> Option<(u32, (f32, f32))> {
    // A track with a chain plays into it even while it is not heard (muted, or another track
    // soloed): the chain's gains are zero then, and its instrument keeps its notes, so a note
    // held through an unmute or a solo sounds at once, as a mute is only a gain in OSS hosts
    // too; leaving the notes out made it wait for the next note.
    if let Some(index) = chains.iter().position(|c| c.track == track) {
        return Some((u32::try_from(index + 1).ok()?, (CHAIN_INPUT_GAIN, CHAIN_INPUT_GAIN)));
    }
    Some((0, track_gains(project, track)?))
}

impl Graph {
    /// Builds the graph for `project`: validates it and prepares every event and voice. Allocates;
    /// call it off the audio thread. The file-backed devices' files come from `assets`.
    pub(crate) fn build(
        project: &Project,
        audio: &AudioPool,
        assets: &crate::DeviceAssets,
        sample_rate: u32,
        plugins: &mut Plugins,
    ) -> Result<Box<Self>, RenderError> {
        if !(8_000..=384_000).contains(&sample_rate) {
            return Err(RenderError::InvalidSampleRate(sample_rate));
        }
        let build_began = std::time::Instant::now();
        let bpm = project.transport.tempo_bpm;
        if !bpm.is_finite() || !(1.0..=999.0).contains(&bpm) {
            return Err(RenderError::InvalidTempo(bpm));
        }
        project.validate().map_err(RenderError::InvalidProject)?;
        // Mixer parameters that follow lanes are built at their neutral values and applied
        // after the track's chain, frame by frame (#395).
        let neutral = automation::neutral(project);
        let project = neutral.as_ref().unwrap_or(project);

        let tempo = Arc::new(project.transport.tempo_map(f64::from(sample_rate)));
        let frames_per_beat = tempo.start_per_beat();
        // Audio events follow the map only when it changes, so a constant tempo plays exactly as
        // it always did.
        let changing = (!tempo.is_constant()).then(|| Arc::clone(&tempo));
        let column_of = |track: TrackId| {
            project
                .launcher
                .columns
                .iter()
                .position(|c| c.track == track)
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(NO_COLUMN)
        };
        // Device chains, group buses and latency compensation: every track is heard as late as
        // the path whose devices delay it most.
        let chains_began = std::time::Instant::now();
        let (chains, latency) = buses::build(project, audio, sample_rate, plugins, column_of, assets);
        let chains_took = chains_began.elapsed();
        let mut voices_took = std::time::Duration::ZERO;
        let mut events = Vec::new();
        let mut audio_events = Vec::new();
        let mut stretch_events = Vec::new();
        let mut stretch_voices = Vec::new();
        for (lane_index, lane) in project.arrangement.lanes.iter().enumerate() {
            for (clip_index, clip) in lane.clips.iter().enumerate().filter(|(_, c)| c.enabled) {
                let Some(content) = project.content(clip.content) else { continue };
                for (content_index, content_lane) in content.lanes.iter().enumerate() {
                    let Some(track) = content_lane.target.or(lane.track) else { continue };
                    let Some((bus, (gain_left, gain_right))) = route(project, &chains, track) else { continue };
                    let column = column_of(track);
                    let meter = meters::meter_slot(project, track);
                    match &content_lane.data {
                        LaneData::Notes(_) => collect_notes(
                            clip,
                            content_lane,
                            &tempo,
                            (track, gain_left, gain_right, column, bus, meter),
                            musical_scale(content.scale),
                            &mut events,
                        ),
                        LaneData::Audio(region) => {
                            let Some(data) = audio.files.get(region.pool_key().as_ref()) else { continue };
                            let gains = (gain_left * AUDIO_HEADROOM, gain_right * AUDIO_HEADROOM);
                            // Warped clips are time-stretched as they play; the rest, and clips
                            // the stretcher cannot handle, are re-pitched.
                            let voice_began = std::time::Instant::now();
                            let voice = (region.warped && region.stretch != suhina_model::StretchMode::Repitch)
                                .then(|| StretchVoice::on_map(clip, region, data.clone(), &tempo, sample_rate))
                                .flatten();
                            voices_took += voice_began.elapsed();
                            if let Some(voice) = voice {
                                let gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
                                let to_frame = |beats: f64| tempo::frame_of(&tempo, beats);
                                let (start, end) = (to_frame(clip.start), to_frame(clip.start + clip.length));
                                stretch_voices.push((voice, start, end.saturating_sub(start)));
                                stretch_events.push(StretchEvent {
                                    start,
                                    end,
                                    gain_left: gains.0 * gain,
                                    gain_right: gains.1 * gain,
                                    fades: Fades::on_map(clip, &tempo, sample_rate),
                                    column,
                                    bus,
                                    meter,
                                });
                                continue;
                            }
                            let timing =
                                audio::Timing { frames_per_beat, tempo_bpm: bpm, sample_rate, map: changing.clone() };
                            audio_events.extend(AudioEvent::new(clip, region, data.clone(), timing, gains).map(
                                |event| ArrangedAudio {
                                    event,
                                    column,
                                    bus,
                                    meter,
                                    key: (lane_index, clip_index, content_index),
                                },
                            ));
                        }
                    }
                }
            }
        }
        events.sort_by_key(|e| e.start);
        audio_events.sort_by_key(|a| a.event.start);

        let release = (RELEASE_SECONDS * f64::from(sample_rate)) as u64;
        let notes_end = events.iter().map(|e| e.end.saturating_add(release)).max().unwrap_or(0);
        let length = audio_events
            .iter()
            .map(|a| a.event.end)
            .chain(stretch_events.iter().map(|e: &StretchEvent| e.end))
            .max()
            .unwrap_or(0)
            .max(notes_end);
        // A passthrough chain (a cued track's, or one into a bus) adds no tail of its own.
        let tail = if chains.iter().all(|c| !c.has_devices()) {
            0
        } else {
            (CHAIN_TAIL_SECONDS * f64::from(sample_rate)) as u64
        };
        let length = if length > 0 { length + tail + u64::from(latency) } else { 0 };
        let seconds = length as f64 / f64::from(sample_rate);
        if seconds > MAX_RENDER_SECONDS {
            return Err(RenderError::TooLong { seconds });
        }

        let inputs: Vec<input::InputRoute> = project
            .tracks
            .iter()
            .filter_map(|t| {
                let (left, right) = match t.audio_input.channels {
                    suhina_model::InputChannels::None => return None,
                    suhina_model::InputChannels::Mono(c) => (usize::from(c.checked_sub(1)?), usize::from(c - 1)),
                    suhina_model::InputChannels::Stereo(c) => (usize::from(c.checked_sub(1)?), usize::from(c)),
                };
                let (bus, (gain_left, gain_right)) = route(project, &chains, t.id)?;
                let gains = (gain_left * AUDIO_HEADROOM, gain_right * AUDIO_HEADROOM);
                Some(input::InputRoute { track: t.id, left, right, monitor: t.audio_input.monitor, bus, gains })
            })
            .collect();
        let rings_began = std::time::Instant::now();
        let voices = stretch_voices.len();
        let ahead = AheadReader::new(stretch_voices, sample_rate);
        let rings_took = rings_began.elapsed();
        let launcher_began = std::time::Instant::now();
        let launcher = Launcher::new(project, audio, sample_rate, &chains, Arc::clone(&tempo));
        let launcher_took = launcher_began.elapsed();
        let levels_began = std::time::Instant::now();
        let levels = meters::Levels::build(project, &chains);
        let levels_took = levels_began.elapsed();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        build_report::set(format!(
            "graph {:.1} ms: before the chains {:.1} ms, device chains {:.1} ms, {voices} stretch voices {:.1} ms, their rings {:.1} ms, launcher {:.1} ms, levels {:.1} ms",
            ms(build_began.elapsed()),
            ms(chains_began.duration_since(build_began)),
            ms(chains_took),
            ms(voices_took),
            ms(rings_took),
            ms(launcher_took),
            ms(levels_took),
        ));
        Ok(Box::new(Graph {
            inputs_heard: vec![false; inputs.len()],
            inputs,
            events,
            audio_events,
            stretch_events,
            ahead,
            length,
            launcher,
            tempo,
            loop_beats: project.transport.looping().map(|r| (r.start, r.end)),
            routes: project
                .tracks
                .iter()
                .map(|t| {
                    let (bus, gains) = route(project, &chains, t.id).unwrap_or((0, (0.0, 0.0)));
                    (t.id, bus, gains)
                })
                .collect(),
            levels,
            chains,
            latency,
            direct_delay: Delay::new(latency as usize),
        }))
    }
}

/// Splits an optional cue buffer at `at` samples, as the main buffer is split; past its end
/// both parts are empty.
fn split_cue(cue: Option<&mut [f32]>, at: usize) -> (Option<&mut [f32]>, Option<&mut [f32]>) {
    match cue {
        Some(cue) => {
            let (before, after) = cue.split_at_mut(at.min(cue.len()));
            (Some(before), Some(after))
        }
        None => (None, None),
    }
}

/// Renders the whole arrangement offline into interleaved stereo, `block_frames` at a time.
/// Audio clips stay silent; see [`render_offline_with_audio`].
pub fn render_offline(project: &Project, sample_rate: u32, block_frames: usize) -> Result<Vec<f32>, RenderError> {
    render_offline_with_audio(project, &AudioPool::new(), sample_rate, block_frames)
}

/// Like [`render_offline`], playing audio clips whose files are in `audio`.
pub fn render_offline_with_audio(
    project: &Project,
    audio: &AudioPool,
    sample_rate: u32,
    block_frames: usize,
) -> Result<Vec<f32>, RenderError> {
    render_offline_with_plugins(project, audio, sample_rate, block_frames, Plugins::new())
}

/// Like [`render_offline_with_audio`], with plug-in devices running in `plugins`.
pub fn render_offline_with_plugins(
    project: &Project,
    audio: &AudioPool,
    sample_rate: u32,
    block_frames: usize,
    plugins: Plugins,
) -> Result<Vec<f32>, RenderError> {
    render_offline_with_assets(project, audio, &DeviceAssets::new(), sample_rate, block_frames, plugins)
}

/// A renderer whose file-backed devices run the parsed models and decoded IRs `assets` holds,
/// and whose plug-in devices run in the processors `plugins`. The same `process` as playback,
/// waiting for each plug-in instead of a real-time deadline.
pub fn render_offline_with_assets(
    project: &Project,
    audio: &AudioPool,
    assets: &DeviceAssets,
    sample_rate: u32,
    block_frames: usize,
    plugins: Plugins,
) -> Result<Vec<f32>, RenderError> {
    let mut renderer = Renderer::with_assets_plugins(project, audio, assets, sample_rate, plugins)?;
    renderer.set_offline(true);
    // The whole arrangement, once: a render does not repeat the loop region.
    renderer.set_follow_loop(false);
    let total = usize::try_from(renderer.length())
        .ok()
        .and_then(|frames| frames.checked_mul(2))
        .ok_or(RenderError::TooLong { seconds: renderer.length() as f64 / f64::from(sample_rate) })?;
    let mut out = vec![0.0f32; total];
    for block in out.chunks_mut(block_frames.max(1).saturating_mul(2)) {
        renderer.process(block);
    }
    Ok(out)
}

/// Renders the whole arrangement offline like [`render_offline_with_plugins`], and the cue bus
/// beside it: the main mix and the cue, both interleaved stereo of the same length. `cue_gain`
/// is the cue volume (linear) and `separate` whether the cue has outputs of its own (then the
/// preview is heard only on the cue, as on the device). The file-backed devices run what
/// `assets` holds ([docs/spec/nam-amp-and-cab.md]).
#[allow(clippy::too_many_arguments)]
pub fn render_offline_with_cue(
    project: &Project,
    audio: &AudioPool,
    assets: &DeviceAssets,
    sample_rate: u32,
    block_frames: usize,
    plugins: Plugins,
    cue_gain: f32,
    separate: bool,
) -> Result<(Vec<f32>, Vec<f32>), RenderError> {
    let mut renderer = Renderer::with_assets_plugins(project, audio, assets, sample_rate, plugins)?;
    renderer.set_offline(true);
    renderer.handle(LaunchCommand::SetCue { gain: Some(cue_gain), separate: Some(separate) });
    let total = usize::try_from(renderer.length())
        .ok()
        .and_then(|frames| frames.checked_mul(2))
        .ok_or(RenderError::TooLong { seconds: renderer.length() as f64 / f64::from(sample_rate) })?;
    let mut out = vec![0.0f32; total];
    let mut cue = vec![0.0f32; total];
    let block = block_frames.max(1).saturating_mul(2);
    for (main, cued) in out.chunks_mut(block).zip(cue.chunks_mut(block)) {
        renderer.process_with_cue(main, cued);
    }
    Ok((out, cue))
}

/// Renders `seconds` of the project offline, starting with the transport stopped and applying
/// each launch command at its time in seconds (in order). This is the same path a live
/// [`Controller`] drives, so launcher behaviour renders offline exactly as it plays.
pub fn render_launches(
    project: &Project,
    audio: &AudioPool,
    sample_rate: u32,
    commands: &[(f64, LaunchCommand)],
    seconds: f64,
) -> Result<Vec<f32>, RenderError> {
    render_launches_with_plugins(project, audio, sample_rate, commands, seconds, Plugins::new())
}

/// Like [`render_launches`], with plug-in devices running in `plugins`.
pub fn render_launches_with_plugins(
    project: &Project,
    audio: &AudioPool,
    sample_rate: u32,
    commands: &[(f64, LaunchCommand)],
    seconds: f64,
    plugins: Plugins,
) -> Result<Vec<f32>, RenderError> {
    if !(seconds.is_finite() && seconds >= 0.0) || seconds > MAX_RENDER_SECONDS {
        return Err(RenderError::TooLong { seconds });
    }
    let mut renderer = Renderer::stopped_with_plugins(project, audio, sample_rate, plugins)?;
    renderer.set_offline(true);
    let frame_of = |at: f64| (at.max(0.0) * f64::from(sample_rate)).round() as usize;
    let total = frame_of(seconds);
    let mut out = vec![0.0f32; total.saturating_mul(2)];
    let mut done = 0;
    let mut pending = commands.iter().peekable();
    while done < total {
        while let Some((_, command)) = pending.next_if(|(at, _)| frame_of(*at) <= done) {
            renderer.handle(*command);
        }
        let next = pending.peek().map_or(total, |(at, _)| frame_of(*at));
        let end = next.clamp(done + 1, total).min(done + 512);
        if let Some(block) = out.get_mut(done * 2..end * 2) {
            renderer.process(block);
        }
        done = end;
    }
    Ok(out)
}

/// Equal-power pan and channel volume for a track; `None` when the track is silent (muted, or
/// it has no channel). The volume and mute of every channel the track's output passes through
/// (groups, then the master) apply too; their pan does not yet.
///
/// Solo: while any channel is soloed, a track sounds only if a channel on its way to the master
/// is soloed (its own, or a group it plays into), or a soloed track plays into it (a group), or
/// it is a return track, so effects stay audible.
/// A note's velocity, 0 … 1; a NaN or infinite one is 0, so it cannot reach the mix.
pub(crate) fn note_velocity(velocity: f32) -> f32 {
    if velocity.is_finite() { velocity.clamp(0.0, 1.0) } else { 0.0 }
}

pub(crate) fn track_gains(project: &Project, track: TrackId) -> Option<(f32, f32)> {
    let channel_id = project.tracks.iter().find(|t| t.id == track)?.channel?;
    let channel = project.channels.iter().find(|c| c.id == channel_id)?;
    let pan = if channel.pan.is_finite() { channel.pan.clamp(-1.0, 1.0) } else { 0.0 };
    let gain = |c: &suhina_model::Channel| if c.volume.is_finite() { c.volume.max(0.0) } else { 0.0 };

    let mut volume = 1.0;
    let mut visited = Vec::new();
    let mut next = Some(channel);
    // Bounded, so a routing cycle in a malformed project cannot loop forever.
    while let Some(c) = next.take() {
        if c.mute || visited.len() > 64 || visited.contains(&c.id) {
            return None;
        }
        visited.push(c.id);
        volume *= gain(c);
        next = c.output.and_then(|id| project.channels.iter().find(|o| o.id == id));
    }
    if !heard_with_solo(project, channel, &visited) {
        return None;
    }
    if let Some(master) = project.channels.iter().find(|c| c.role == ChannelRole::Master && !visited.contains(&c.id)) {
        if master.mute {
            return None;
        }
        volume *= gain(master);
    }
    let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
    // Scaled so a centred note has unity gain on each side, and quieter overall to leave headroom.
    let scale = volume * std::f64::consts::SQRT_2 * 0.25;
    Some(((angle.cos() * scale) as f32, (angle.sin() * scale) as f32))
}

/// Whether a track whose output passes through `chain` is heard while channels are soloed.
fn heard_with_solo(project: &Project, channel: &suhina_model::Channel, chain: &[ChannelId]) -> bool {
    let soloed = |c: &suhina_model::Channel| c.solo && c.role != ChannelRole::Master;
    if !project.channels.iter().any(soloed) || channel.role == ChannelRole::Effect {
        return true;
    }
    let on_chain = project.channels.iter().any(|c| soloed(c) && chain.contains(&c.id));
    // A group sounds when something soloed plays into it.
    let feeds_it = project.channels.iter().filter(|c| soloed(c)).any(|c| {
        let mut next = c.output;
        for _ in 0..64 {
            match next {
                Some(id) if id == channel.id => return true,
                Some(id) => next = project.channels.iter().find(|o| o.id == id).and_then(|o| o.output),
                None => return false,
            }
        }
        false
    });
    on_chain || feeds_it
}

/// Adds the notes of one content lane, as placed by `clip`, to `events`. Notes outside the clip's
/// window are skipped; with a loop range the loop repeats until the clip ends.
fn collect_notes(
    clip: &Clip,
    lane: &ContentLane,
    map: &TempoMap,
    (track, gain_left, gain_right, column, bus, meter): (TrackId, f32, f32, u32, u32, u32),
    scale: Option<MusicalScale>,
    events: &mut Vec<NoteEvent>,
) {
    let LaneData::Notes(notes) = &lane.data else { return };
    if !(clip.length.is_finite() && clip.length > 0.0 && clip.start.is_finite() && clip.offset.is_finite()) {
        return;
    }
    let to_frame = |beats: f64| tempo::frame_of(map, beats);

    // Each pass maps the content range [from, to) to the clip position `at`.
    let mut push_range = |from: f64, to: f64, at: f64| {
        for note in notes {
            // A muted note stays in the clip and produces no event at all, exactly as if it were
            // not there (ADR 0001 › Amendment: muted notes).
            if note.muted {
                continue;
            }
            if !(note.time.is_finite() && note.duration.is_finite()) || note.time < from || note.time >= to {
                continue;
            }
            let start = clip.start + at + (note.time - from);
            let clip_end = clip.start + clip.length;
            if start >= clip_end {
                continue;
            }
            let end = (start + note.duration.max(0.0)).min(clip_end);
            events.push(NoteEvent {
                start: to_frame(start),
                end: to_frame(end),
                frequency: 440.0 * 2f64.powf((f64::from(note.key) - 69.0) / 12.0),
                key: note.key.min(127),
                velocity: note_velocity(note.velocity),
                track,
                gain_left: gain_left * note_velocity(note.velocity),
                gain_right: gain_right * note_velocity(note.velocity),
                column,
                bus,
                meter,
                scale,
            });
        }
    };

    match clip.loop_range {
        Some(range) if range.end.is_finite() && range.start.is_finite() && range.end > range.start => {
            let loop_len = range.end - range.start;
            // First pass from the offset to the loop end, then whole loops until the clip ends.
            let first_from = clip.offset.clamp(range.start, range.end);
            push_range(first_from, range.end, 0.0);
            let mut at = range.end - first_from;
            let max_passes = 100_000;
            let mut passes = 0;
            while at < clip.length && passes < max_passes {
                push_range(range.start, range.end, at);
                at += loop_len;
                passes += 1;
            }
        }
        _ => push_range(clip.offset, f64::INFINITY, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::*;

    const SR: u32 = 48_000;

    fn note(time: f64, duration: f64, key: u8) -> Note {
        Note { time, duration, key, velocity: 1.0, release_velocity: 0.5, channel: 0, muted: false }
    }

    fn project_with(lanes: Vec<ContentLane>, clips: Vec<Clip>, tracks: u32) -> Project {
        Project {
            tracks: (1..=tracks)
                .map(|i| Track {
                    id: TrackId(i),
                    name: format!("T{i}"),
                    color: None,
                    content: ContentKinds { notes: true, audio: false },
                    channel: Some(ChannelId(i)),
                    parent: None,
                    midi_input: Default::default(),
                    audio_input: Default::default(),
                    extensions: Extensions::default(),
                })
                .collect(),
            channels: (1..=tracks)
                .map(|i| Channel {
                    id: ChannelId(i),
                    name: format!("C{i}"),
                    role: ChannelRole::Regular,
                    volume: 1.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    cue: false,
                    output: None,
                    sends: Vec::new(),
                    devices: Vec::new(),
                    extensions: Extensions::default(),
                    automation: Vec::new(),
                })
                .collect(),
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "c".into(),
                length: 4.0,
                lanes,
                extensions: Extensions::default(),
                scale: None,
            }],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: "l".into(),
                    track: Some(TrackId(1)),
                    clips,
                }],
                markers: Vec::new(),
            },
            ..Project::default()
        }
    }

    fn clip(start: f64, length: f64) -> Clip {
        Clip {
            id: Default::default(),
            content: ContentId(1),
            start,
            length,
            offset: 0.0,
            loop_range: None,
            enabled: true,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
            follow_action: Default::default(),
            launch: Default::default(),
            color: None,
            extensions: Extensions::default(),
        }
    }

    fn untargeted(notes: Vec<Note>) -> Vec<ContentLane> {
        vec![ContentLane { target: None, data: LaneData::Notes(notes) }]
    }

    /// Frames where sound starts after more than 100 silent frames.
    fn onsets(out: &[f32]) -> Vec<usize> {
        let mut result = Vec::new();
        let mut silent_run = usize::MAX;
        for (i, frame) in out.as_chunks::<2>().0.iter().enumerate() {
            let loud = frame.iter().any(|s| s.abs() > 1e-6);
            if loud && silent_run > 100 {
                result.push(i);
            }
            silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
        }
        result
    }

    /// A sine starts at phase zero, so the first audible frame lands a frame or two after note-on.
    fn assert_onsets(out: &[f32], expected: &[usize]) {
        let actual = onsets(out);
        assert_eq!(actual.len(), expected.len(), "onsets {actual:?}, expected {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!(a.abs_diff(*e) <= 2, "onsets {actual:?}, expected {expected:?}");
        }
    }

    #[test]
    fn empty_project_renders_nothing() {
        assert_eq!(render_offline(&Project::default(), SR, 256), Ok(Vec::new()));
    }

    #[test]
    fn note_on_beat_two_starts_half_a_second_in_at_120_bpm() {
        let project = project_with(untargeted(vec![note(1.0, 1.0, 69)]), vec![clip(0.0, 4.0)], 1);
        let out = render_offline(&project, SR, 256).unwrap();
        assert_onsets(&out, &[SR as usize / 2]);
    }

    #[test]
    fn a4_plays_at_440_hz() {
        let project = project_with(untargeted(vec![note(0.0, 2.0, 69)]), vec![clip(0.0, 4.0)], 1);
        let out = render_offline(&project, SR, 256).unwrap();
        // Count upward zero crossings over one second of the left channel.
        let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|f| f[0]).take(SR as usize).collect();
        let crossings = left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((439..=441).contains(&crossings), "{crossings} crossings");
    }

    #[test]
    fn block_size_does_not_change_the_output() {
        let project = project_with(
            untargeted(vec![note(0.0, 0.5, 60), note(0.25, 1.0, 64), note(1.5, 0.3, 67)]),
            vec![clip(0.0, 4.0)],
            1,
        );
        let reference = render_offline(&project, SR, 1).unwrap();
        for block in [7, 64, 256, 1024, 100_000] {
            assert_eq!(render_offline(&project, SR, block).unwrap(), reference, "block {block}");
        }
    }

    #[test]
    fn muted_channel_is_silent() {
        let mut project = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![clip(0.0, 4.0)], 1);
        project.channels[0].mute = true;
        assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
    }

    /// Mutes note `index` of the first content's first note lane.
    fn mute_note(project: &mut Project, index: usize) {
        if let Some(lane) = project.contents.first_mut().and_then(|c| c.lanes.first_mut())
            && let LaneData::Notes(notes) = &mut lane.data
            && let Some(note) = notes.get_mut(index)
        {
            note.muted = true;
        }
    }

    #[test]
    fn a_muted_note_is_silent_and_does_not_shorten_the_render() {
        // The long first note covers the short second one's span, so the render lasts the same
        // whether the second note plays or not.
        let notes = vec![note(0.0, 2.0, 60), note(0.5, 0.5, 64)];
        let mut project = project_with(untargeted(notes.clone()), vec![clip(0.0, 4.0)], 1);
        let all = render_offline(&project, SR, 256).unwrap();
        let ends = SR as usize + (RELEASE_SECONDS * f64::from(SR)) as usize;
        assert_eq!(all.len() / 2, ends);

        mute_note(&mut project, 1);
        let muted = render_offline(&project, SR, 256).unwrap();
        assert_eq!(muted.len(), all.len(), "muting does not change the render length");
        assert!(rms(&muted, 0, muted.len() / 2) < rms(&all, 0, all.len() / 2));
        // The muted note is not there at all: the render is the other note's alone.
        let alone = project_with(untargeted(vec![notes[0]]), vec![clip(0.0, 4.0)], 1);
        assert_eq!(muted, render_offline(&alone, SR, 256).unwrap());

        // Every note muted: silence, and no length at all.
        mute_note(&mut project, 0);
        assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
    }

    #[test]
    fn muting_one_of_two_notes_halves_the_sounding_notes() {
        // Two notes sounding together: muting one leaves a single voice, so its RMS is about
        // 1/sqrt(2) of the pair's, and the render is what the other note alone would give.
        let notes = vec![note(0.0, 1.0, 60), note(0.0, 1.0, 64)];
        let mut project = project_with(untargeted(notes.clone()), vec![clip(0.0, 4.0)], 1);
        let all = render_offline(&project, SR, 256).unwrap();
        let all_rms = rms(&all, 0, all.len() / 2);

        mute_note(&mut project, 0);
        let muted = render_offline(&project, SR, 256).unwrap();
        let muted_rms = rms(&muted, 0, muted.len() / 2);
        assert!(
            muted_rms < all_rms * 0.9 && muted_rms > all_rms * 0.5,
            "one of two voices: {muted_rms} against {all_rms}"
        );

        let alone = project_with(untargeted(vec![notes[1]]), vec![clip(0.0, 4.0)], 1);
        assert_eq!(muted, render_offline(&alone, SR, 256).unwrap(), "the muted note is not there");
        assert_eq!(onsets(&all).len(), 1, "the two notes start together");
    }

    #[test]
    fn hard_left_pan_leaves_right_silent() {
        let mut project = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![clip(0.0, 4.0)], 1);
        project.channels[0].pan = -1.0;
        let out = render_offline(&project, SR, 256).unwrap();
        assert!(out.as_chunks::<2>().0.iter().all(|f| f[1].abs() < 1e-6));
        assert!(out.as_chunks::<2>().0.iter().any(|f| f[0].abs() > 0.1));
    }

    #[test]
    fn clip_cuts_notes_at_its_end_and_skips_notes_after_it() {
        let project = project_with(untargeted(vec![note(0.0, 4.0, 60), note(2.0, 1.0, 64)]), vec![clip(0.0, 1.0)], 1);
        let out = render_offline(&project, SR, 256).unwrap();
        // One beat at 120 bpm plus the release tail.
        let expected = SR as usize / 2 + (RELEASE_SECONDS * f64::from(SR)) as usize;
        assert_eq!(out.len() / 2, expected);
        assert_eq!(onsets(&out).len(), 1);
    }

    #[test]
    fn loop_range_repeats_until_the_clip_ends() {
        let mut looped = clip(0.0, 4.0);
        looped.loop_range = Some(LoopRange { start: 0.0, end: 1.0 });
        let project = project_with(untargeted(vec![note(0.0, 0.25, 60)]), vec![looped], 1);
        let out = render_offline(&project, SR, 256).unwrap();
        let beat = SR as usize / 2;
        assert_onsets(&out, &[0, beat, 2 * beat, 3 * beat]);
    }

    /// A shared pattern drives two tracks and is placed twice on a free lane.
    #[test]
    fn shared_pattern_plays_every_placement_on_its_target_tracks() {
        let lanes = vec![
            ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 0.25, 36)]) },
            ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(0.5, 0.25, 38)]) },
        ];
        let mut project = project_with(lanes, vec![clip(0.0, 1.0), clip(2.0, 1.0)], 2);
        project.arrangement.lanes[0].track = None;
        let out = render_offline(&project, SR, 256).unwrap();
        let beat = SR as usize / 2;
        assert_onsets(&out, &[0, beat / 2, 2 * beat, 2 * beat + beat / 2]);

        // Muting track 2 removes only its notes.
        project.channels[1].mute = true;
        let out = render_offline(&project, SR, 256).unwrap();
        assert_onsets(&out, &[0, 2 * beat]);
    }

    #[test]
    fn solo_leaves_only_soloed_tracks_their_groups_and_returns() {
        let lanes = vec![
            ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 0.25, 36)]) },
            ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(0.5, 0.25, 38)]) },
        ];
        let mut project = project_with(lanes, vec![clip(0.0, 1.0)], 2);
        project.arrangement.lanes[0].track = None;
        let beat = SR as usize / 2;

        project.channels[1].solo = true;
        assert_onsets(&render_offline(&project, SR, 256).unwrap(), &[beat / 2]);

        // A return track stays audible.
        project.channels[0].role = ChannelRole::Effect;
        assert_onsets(&render_offline(&project, SR, 256).unwrap(), &[0, beat / 2]);
        project.channels[0].role = ChannelRole::Regular;

        // Soloing a group plays what is routed into it.
        project.channels[1].solo = false;
        project.channels.push(Channel { id: ChannelId(5), solo: true, ..project.channels[1].clone() });
        project.channels[0].output = Some(ChannelId(5));
        assert_onsets(&render_offline(&project, SR, 256).unwrap(), &[0]);

        // Muting still wins over solo.
        project.channels[0].mute = true;
        assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
    }

    #[test]
    fn group_and_master_volume_and_mute_apply_to_the_tracks_routed_through_them() {
        let mut project = project_with(untargeted(vec![note(0.0, 1.0, 69)]), vec![clip(0.0, 4.0)], 2);
        let peak = |p: &Project| render_offline(p, SR, 256).unwrap().iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let alone = peak(&project);

        // Track 1 plays into channel 2 (a group) at half volume.
        project.channels[0].output = Some(ChannelId(2));
        project.channels[1].volume = 0.5;
        assert!((peak(&project) - alone * 0.5).abs() < 1e-3);

        project.channels[1].mute = true;
        assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
        project.channels[1].mute = false;

        // A master channel scales everything that does not route to it explicitly.
        project.channels.push(Channel {
            id: ChannelId(9),
            role: ChannelRole::Master,
            volume: 0.5,
            ..project.channels[1].clone()
        });
        assert!((peak(&project) - alone * 0.25).abs() < 1e-3);

        // A routing cycle is silence, not a hang.
        project.channels[1].output = Some(ChannelId(1));
        assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
    }

    /// A warped audio clip of a sine at `hz` (44.1 kHz file) whose warp markers put `seconds` of
    /// audio on `beats` beats, played by a 48 kHz renderer at `bpm`.
    fn stretched_project(hz: f64, seconds: f64, beats: f64, bpm: f64, mode: StretchMode) -> (Project, AudioPool) {
        let file_rate = 44_100u32;
        let frames = (seconds * f64::from(file_rate)) as usize;
        let samples = (0..frames)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(file_rate)).sin() as f32 * 0.5)
            .collect();
        let data = AudioData { sample_rate: file_rate, channels: 1, samples };
        let region = AudioRegion {
            file: "sine.wav".into(),
            absolute_file: None,
            sample_rate: Some(file_rate),
            channels: Some(1),
            duration_seconds: Some(seconds),
            warped: true,
            stretch: mode,
            warp_markers: vec![
                WarpMarker { time: 0.0, audio_time: 0.0 },
                WarpMarker { time: beats, audio_time: seconds },
            ],
            gain: 1.0,
            transpose: 0.0,
            stream: None,
        };
        let mut project =
            project_with(vec![ContentLane { target: None, data: LaneData::Audio(region) }], vec![clip(0.0, beats)], 1);
        project.transport.tempo_bpm = bpm;
        let mut pool = AudioPool::new();
        pool.files.insert("sine.wav".into(), std::sync::Arc::new(data));
        (project, pool)
    }

    /// Upward zero crossings per second over the middle half of the left channel.
    fn frequency(out: &[f32]) -> f64 {
        let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        let middle = &left[left.len() / 4..left.len() * 3 / 4];
        let ups = middle.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        ups as f64 / (middle.len() as f64 / f64::from(SR))
    }

    #[test]
    fn warped_audio_is_stretched_in_time_not_pitch() {
        // 4 s of audio on 8 beats at 100 bpm lasts 4.8 s: 0.83× speed, pitch must stay 440 Hz.
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 100.0, StretchMode::Smooth);
        let out = render_offline_with_audio(&project, &pool, SR, 512).unwrap();
        assert!((out.len() as f64 / 2.0 / f64::from(SR) - 4.8).abs() < 0.01);
        let hz = frequency(&out);
        assert!((hz - 440.0).abs() < 440.0 * 0.01, "{hz} Hz");

        // Re-pitch for comparison: the same clip plays 0.83× lower.
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 100.0, StretchMode::Repitch);
        let hz = frequency(&render_offline_with_audio(&project, &pool, SR, 512).unwrap());
        assert!((hz - 440.0 * 100.0 / 120.0).abs() < 440.0 * 0.01, "{hz} Hz");
    }

    #[test]
    fn rhythmic_mode_keeps_pitch_within_its_range() {
        // 2 s on 4 beats at 126 bpm: 1.05× speed.
        let (project, pool) = stretched_project(330.0, 2.0, 4.0, 126.0, StretchMode::Rhythmic);
        let hz = frequency(&render_offline_with_audio(&project, &pool, SR, 512).unwrap());
        assert!((hz - 330.0).abs() < 330.0 * 0.02, "{hz} Hz");
    }

    #[test]
    fn stretched_audio_does_not_depend_on_the_block_size() {
        let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
        let reference = render_offline_with_audio(&project, &pool, SR, 1024).unwrap();
        for block in [64, 333, 4096] {
            let out = render_offline_with_audio(&project, &pool, SR, block).unwrap();
            assert_eq!(out.len(), reference.len());
            let worst = out.iter().zip(&reference).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst < 1e-4, "block {block}: differs by {worst}");
        }
    }

    /// A NaN velocity is silence for its note, not NaN in the whole mix (#161).
    #[test]
    fn a_nan_velocity_note_leaves_the_mix_finite() {
        let mut bad = note(0.0, 1.0, 60);
        bad.velocity = f32::NAN;
        let project = project_with(untargeted(vec![bad, note(1.0, 1.0, 69)]), vec![clip(0.0, 4.0)], 1);
        let out = render_offline(&project, SR, 256).unwrap();
        assert!(out.iter().all(|s| s.is_finite()));
        assert_onsets(&out, &[SR as usize / 2]);
    }

    #[test]
    fn hostile_input_returns_errors_or_silence_instead_of_panicking() {
        assert_eq!(render_offline(&Project::default(), 0, 256).err(), Some(RenderError::InvalidSampleRate(0)));
        let mut project = Project::default();
        project.transport.tempo_bpm = f64::NAN;
        assert!(matches!(render_offline(&project, SR, 256), Err(RenderError::InvalidTempo(_))));

        let weird = vec![note(f64::NAN, 1.0, 60), note(0.0, f64::INFINITY, 127), note(-5.0, 1.0, 0)];
        let mut project = project_with(untargeted(weird), vec![clip(0.0, 1.0)], 1);
        project.channels[0].volume = f64::NAN;
        assert!(render_offline(&project, SR, 256).is_ok());

        let project = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![clip(0.0, 1e12)], 1);
        let mut looped = clip(0.0, 1e12);
        looped.loop_range = Some(LoopRange { start: 0.0, end: 1e-9 });
        let looping = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![looped], 1);
        assert!(render_offline(&project, SR, 256).is_ok());
        assert!(render_offline(&looping, SR, 256).is_ok());
    }

    /// Renders `blocks` blocks of `frames` from `renderer`, calling `between` before each.
    fn blocks(renderer: &mut Renderer, blocks: usize, frames: usize, mut between: impl FnMut(usize)) -> Vec<f32> {
        let mut out = Vec::new();
        let mut block = vec![0.0f32; frames * 2];
        for i in 0..blocks {
            between(i);
            renderer.process(&mut block);
            out.extend_from_slice(&block);
        }
        out
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
    }

    #[test]
    fn a_stretch_worker_renders_the_same_audio_as_offline() {
        let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
        let mut offline = Renderer::with_audio(&project, &pool, SR).unwrap();
        let reference = blocks(&mut offline, 200, 441, |_| {});
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut worker = renderer.take_stretch_worker().unwrap();
        assert!(renderer.take_stretch_worker().is_none());
        let out = blocks(&mut renderer, 200, 441, |_| worker.fill());
        assert!(reference.iter().any(|s| s.abs() > 0.1));
        assert_eq!(max_diff(&reference, &out), 0.0);
        assert_eq!(renderer.stretch_underruns(), 0);
    }

    #[test]
    fn a_late_stretch_worker_costs_silence_but_not_timing() {
        let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
        let mut offline = Renderer::with_audio(&project, &pool, SR).unwrap();
        let reference = blocks(&mut offline, 200, 441, |_| {});
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut worker = renderer.take_stretch_worker().unwrap();
        // The worker stalls for 1 s (blocks 0-99), longer than it renders ahead.
        let out = blocks(&mut renderer, 200, 441, |i| {
            if i >= 100 {
                worker.fill();
            }
        });
        assert!(renderer.stretch_underruns() > 0);
        let stalled = &out[2 * 441 * 70..2 * 441 * 100];
        assert!(stalled.iter().all(|s| *s == 0.0), "a stalled worker plays silence");
        // Once it catches up the clip plays where it should, not where it left off: the voice
        // jumped ahead of the playhead rather than rendering the second it missed, so it sounds
        // again within a few blocks, as loud as the clip is there. Its stretcher started again
        // at the jump, so the samples are not the reference's to the bit.
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
        let block = |samples: &[f32], i: usize| samples[2 * 441 * i..2 * 441 * (i + 1)].to_vec();
        for i in 115..200 {
            let (heard, wanted) = (rms(&block(&out, i)), rms(&block(&reference, i)));
            assert!((heard - wanted).abs() <= wanted * 0.1 + 1e-3, "block {i}: {heard} against {wanted}");
        }
    }

    #[test]
    fn a_stretch_worker_rewinds_with_the_transport() {
        let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut worker = renderer.take_stretch_worker().unwrap();
        let first = blocks(&mut renderer, 50, 441, |_| worker.fill());
        renderer.handle(LaunchCommand::Stop);
        // The rewind handshake: the worker pauses, the audio side empties the rings, the worker
        // rewinds and fills them again, all while the transport is stopped.
        blocks(&mut renderer, 3, 441, |_| worker.fill());
        renderer.handle(LaunchCommand::Play);
        let again = blocks(&mut renderer, 50, 441, |_| worker.fill());
        assert_eq!(max_diff(&first, &again), 0.0);
        assert_eq!(renderer.stretch_underruns(), 0);
    }

    /// A jump of the playhead while playing is silent until the worker has moved its voices, but
    /// that silence is intended, not an underrun (#169). Two block sizes, since the engine's
    /// block follows the device's buffer.
    #[test]
    fn a_transport_jump_is_not_counted_as_underruns() {
        let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
        for frames in [128, 441] {
            let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
            let mut worker = renderer.take_stretch_worker().unwrap();
            blocks(&mut renderer, 20, frames, |_| worker.fill());
            renderer.handle(LaunchCommand::SetPosition { beat: 1.0 });
            let after = blocks(&mut renderer, 40, frames, |_| worker.fill());
            assert!(after.iter().any(|s| s.abs() > 0.1), "the clip plays on from the new place");
            assert_eq!(renderer.stretch_underruns(), 0, "blocks of {frames}");
        }
    }

    #[test]
    fn a_stretch_worker_thread_keeps_up() {
        let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
        let mut offline = Renderer::with_audio(&project, &pool, SR).unwrap();
        let reference = blocks(&mut offline, 100, 441, |_| {});
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        controller.run_stretch_worker(&mut renderer).unwrap();
        // Paced like a device: one 10 ms block every 10 ms.
        let out = blocks(&mut renderer, 100, 441, |_| std::thread::sleep(std::time::Duration::from_millis(10)));
        assert_eq!(controller.stretch_underruns(), 0);
        assert_eq!(max_diff(&reference, &out), 0.0);
    }

    /// RMS of the left channel over frames `from..to`.
    fn rms(out: &[f32], from: usize, to: usize) -> f64 {
        let left: Vec<f64> = out[from * 2..to * 2].iter().step_by(2).map(|s| f64::from(*s)).collect();
        (left.iter().map(|s| s * s).sum::<f64>() / left.len() as f64).sqrt()
    }

    /// Frame where an update sent after `blocks` blocks of 480 frames takes over, for a project
    /// with stretched clips; one without takes over at the next block, `blocks * 480`.
    fn swap_frame(blocks: usize) -> usize {
        blocks * 480 + (0.15 * f64::from(SR)) as usize
    }

    #[test]
    fn an_edit_is_heard_from_the_frame_it_takes_over() {
        // Read directly (re-pitch), and stretched.
        for mode in [StretchMode::Repitch, StretchMode::Smooth] {
            let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, mode);
            let mut edited = project.clone();
            edited.channels[0].volume = 0.5;
            let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
            let mut controller = renderer.controller();
            let mut out = blocks(&mut renderer, 50, 480, |_| {});
            controller.update(&edited).unwrap();
            out.extend(blocks(&mut renderer, 100, 480, |_| {}));
            // Read directly, there are no stretch voices to prepare: the edit takes the next block.
            let at = if mode == StretchMode::Repitch { 50 * 480 } else { swap_frame(50) };
            let ratio = rms(&out, at, at + 9_600) / rms(&out, at - 9_600, at);
            assert!((ratio - 0.5).abs() < 0.01, "{mode:?}: level changed by {ratio}");
            let mut original = Renderer::with_audio(&project, &pool, SR).unwrap();
            let original = blocks(&mut original, 150, 480, |_| {});
            assert_eq!(max_diff(&out[..at * 2], &original[..at * 2]), 0.0, "{mode:?}: before the swap");
            if mode == StretchMode::Repitch {
                // Read directly, the edited project's own render is the oracle, sample for sample.
                let mut reference = Renderer::with_audio(&edited, &pool, SR).unwrap();
                let reference = blocks(&mut reference, 150, 480, |_| {});
                assert_eq!(max_diff(&out[at * 2..], &reference[at * 2..]), 0.0);
            }
        }
    }

    #[test]
    fn the_playhead_keeps_its_beat_when_the_tempo_changes() {
        // A short note on every beat, at 120 bpm (24 000 frames per beat).
        let notes = (0..8).map(|b| note(f64::from(b), 0.05, 69)).collect();
        let project = project_with(untargeted(notes), vec![clip(0.0, 8.0)], 1);
        let mut slower = project.clone();
        slower.transport.tempo_bpm = 60.0;
        let mut renderer = Renderer::with_audio(&project, &AudioPool::new(), SR).unwrap();
        let mut controller = renderer.controller();
        let mut out = blocks(&mut renderer, 90, 480, |_| {});
        controller.update(&slower).unwrap();
        out.extend(blocks(&mut renderer, 285, 480, |_| {}));
        // With no stretched clips the swap takes the next block: frame 43 200, beat 1.8; beat 2
        // then comes 0.2 beats of 60 bpm later.
        let at = 90 * 480;
        assert_eq!(at, 43_200);
        let expected = [1, 24_001, at + 9_601, at + 9_601 + 48_000, at + 9_601 + 96_000];
        assert_eq!(onsets(&out)[..5], expected);
    }

    #[test]
    fn a_launched_clip_plays_on_through_an_edit() {
        let (mut project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
        // The clip goes to the launcher instead of the arrangement; a second, silent track gets
        // the edit.
        let looped = Clip {
            loop_range: Some(LoopRange { start: 0.0, end: 8.0 }),
            ..project.arrangement.lanes[0].clips[0].clone()
        };
        project.arrangement.lanes[0].clips.clear();
        project.launcher.columns =
            vec![LauncherColumn { track: TrackId(1), slots: vec![Slot { clip: Some(looped), has_stop: true }] }];
        project.launcher.scenes = vec![Scene::default()];
        let mut edited = project.clone();
        edited.transport.time_signature = project.transport.time_signature;
        edited.tracks[0].name = "renamed".into();
        let render = |update: bool| {
            let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
            let mut controller = renderer.controller();
            renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
            let mut out = blocks(&mut renderer, 50, 480, |_| {});
            if update {
                controller.update(&edited).unwrap();
            }
            out.extend(blocks(&mut renderer, 100, 480, |_| {}));
            out
        };
        let (plain, edited_out) = (render(false), render(true));
        assert!(rms(&plain, 30_000, 40_000) > 0.1);
        // The playing clip keeps its stretch voice: not a sample differs.
        assert_eq!(max_diff(&plain, &edited_out), 0.0);
    }

    #[test]
    fn an_edit_while_stopped_is_heard_from_the_next_start() {
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
        let mut edited = project.clone();
        edited.channels[0].volume = 0.25;
        let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        controller.update(&edited).unwrap();
        blocks(&mut renderer, 2, 480, |_| {});
        renderer.handle(LaunchCommand::Play);
        let out = blocks(&mut renderer, 50, 480, |_| {});
        let mut reference = Renderer::with_audio(&edited, &pool, SR).unwrap();
        let reference = blocks(&mut reference, 50, 480, |_| {});
        assert_eq!(max_diff(&out, &reference), 0.0);
    }

    #[test]
    fn stretched_clips_carry_on_through_a_tempo_edit() {
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
        let mut slower = project.clone();
        slower.transport.tempo_bpm = 95.0;
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        controller.run_stretch_worker(&mut renderer).unwrap();
        // Paced like a device, with the worker on its own thread.
        let pace = |_| std::thread::sleep(std::time::Duration::from_millis(10));
        let mut out = blocks(&mut renderer, 50, 480, pace);
        controller.update(&slower).unwrap();
        out.extend(blocks(&mut renderer, 100, 480, pace));
        // The new graph's stretch voices were ready where the playhead went: no gap.
        assert_eq!(renderer.stretch_underruns(), 0);
        let at = swap_frame(50);
        assert!(rms(&out, at, at + 4_800) > 0.9 * rms(&out, at - 4_800, at));
    }

    #[test]
    fn an_unwarped_clip_carries_on_through_a_tempo_edit() {
        // Unwarped audio plays at its own speed: a tempo edit must not move it in its file.
        let (mut project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
        for content in &mut project.contents {
            for lane in &mut content.lanes {
                if let LaneData::Audio(region) = &mut lane.data {
                    region.warped = false;
                }
            }
        }
        let mut slower = project.clone();
        slower.transport.tempo_bpm = 90.0;
        let mut original = Renderer::with_audio(&project, &pool, SR).unwrap();
        let original = blocks(&mut original, 150, 480, |_| {});
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        let mut out = blocks(&mut renderer, 50, 480, |_| {});
        controller.update(&slower).unwrap();
        out.extend(blocks(&mut renderer, 100, 480, |_| {}));
        assert!(rms(&out, 40_000, 70_000) > 0.1);
        // The same file positions as without the edit (to rounding).
        assert!(max_diff(&out, &original) < 1e-6);
    }

    /// Zero crossings per second of the left channel over frames `from..to`.
    fn hz_between(out: &[f32], from: usize, to: usize) -> f64 {
        let left: Vec<f32> = out[from * 2..to * 2].iter().step_by(2).copied().collect();
        let ups = left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        ups as f64 / ((to - from) as f64 / f64::from(SR))
    }

    #[test]
    fn the_metronome_clicks_on_every_beat_and_marks_the_bar() {
        // An empty project in 3/4 at 120 bpm: 24 000 frames to a beat.
        let mut project = Project::default();
        project.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
        renderer.handle(LaunchCommand::Play);
        let out = blocks(&mut renderer, 200, 480, |_| {});
        assert_eq!(onsets(&out), [1, 24_001, 48_001, 72_001]);
        // The bar's first beat is higher than the others.
        let (bar, beat) = (hz_between(&out, 0, 2_000), hz_between(&out, 24_000, 26_000));
        assert!((bar - 1_600.0).abs() < 50.0 && (beat - 1_000.0).abs() < 50.0, "{bar} Hz, {beat} Hz");
        assert!(rms(&out, 0, 2_400) > 0.05);
        // Off: silence, including through a restart.
        renderer.handle(LaunchCommand::SetMetronome { on: false, gain: None });
        renderer.handle(LaunchCommand::Stop);
        renderer.handle(LaunchCommand::Play);
        assert!(blocks(&mut renderer, 200, 480, |_| {}).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_metronome_follows_a_tempo_edit() {
        let mut project = Project::default();
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        let mut controller = renderer.controller();
        renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
        renderer.handle(LaunchCommand::Play);
        let mut out = blocks(&mut renderer, 90, 480, |_| {});
        project.transport.tempo_bpm = 60.0;
        controller.update(&project).unwrap();
        out.extend(blocks(&mut renderer, 285, 480, |_| {}));
        // As with notes: the swap at frame 43 200 is beat 1.8; beat 2 comes 0.2 s later at 60 bpm.
        // The beat clock adds up per frame, so a click may land a frame late.
        let found = onsets(&out);
        for (got, expected) in found.iter().zip([1, 24_001, 52_801, 100_801]) {
            assert!(got.abs_diff(expected) <= 1, "{found:?}");
        }
    }

    fn with_utility(project: &mut Project) {
        project.channels[0].devices.push(Device {
            name: "Utility".into(),
            kind: DeviceKind::Builtin("utility".into()),
            enabled: true,
            state: None,
            params: Vec::new(),
            automation: Vec::new(),
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn edits_sent_together_all_take_the_next_block() {
        // Audio read directly: a volume edit is heard on the playing clip at once.
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Repitch);
        let mut half = project.clone();
        half.channels[0].volume = 0.5;
        let mut quarter = project.clone();
        quarter.channels[0].volume = 0.25;
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        let first = blocks(&mut renderer, 20, 480, |_| {});
        controller.update(&half).unwrap();
        controller.update(&quarter).unwrap();
        // One long block: both graphs are taken at its first frame, the second one winning.
        let next = blocks(&mut renderer, 1, 24_000, |_| {});
        let ratio = rms(&next, 0, 24_000) / rms(&first, 4_800, 9_600);
        assert!((ratio - 0.25).abs() < 0.01, "{ratio}");
    }

    #[test]
    fn a_device_parameter_is_heard_from_the_next_block_and_outlives_an_older_graph() {
        // Stretched audio, so a new graph waits 0.15 s before it takes over.
        let (mut project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
        with_utility(&mut project);
        let mut renderer = Renderer::with_audio(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        let mut out = blocks(&mut renderer, 50, 480, |_| {});
        // A graph built from the project before the change (here with no other edit), then the
        // parameter: -6 dB on the utility's gain, index 0.
        controller.update(&project).unwrap();
        controller.set_device_param(TrackId(1), 0, 0, -6.0206).unwrap();
        out.extend(blocks(&mut renderer, 100, 480, |_| {}));
        let mut untouched = Renderer::with_audio(&project, &pool, SR).unwrap();
        let reference = blocks(&mut untouched, 150, 480, |_| {});
        let now = 50 * 480;
        // Heard from the next block, after the utility's 20 ms ramp...
        let soon = rms(&out, now + 1_200, now + 6_000) / rms(&reference, now + 1_200, now + 6_000);
        assert!((soon - 0.5).abs() < 0.01, "{soon}");
        // ... and still after the older graph took over at 0.15 s.
        let at = swap_frame(50);
        let later = rms(&out, at + 2_400, at + 12_000) / rms(&reference, at + 2_400, at + 12_000);
        assert!((later - 0.5).abs() < 0.01, "{later}");
    }

    #[test]
    fn a_mix_edit_reaches_a_sounding_note_at_the_next_block() {
        let project = project_with(untargeted(vec![note(0.0, 8.0, 69)]), vec![clip(0.0, 8.0)], 1);
        for (edit, expected) in [
            ((0.5, 0.0, false), (0.5, 0.5)),
            ((1.0, 1.0, false), (0.0, std::f64::consts::SQRT_2)),
            ((1.0, 0.0, true), (0.0, 0.0)),
        ] {
            let mut edited = project.clone();
            (edited.channels[0].volume, edited.channels[0].pan, edited.channels[0].mute) = edit;
            let mut renderer = Renderer::with_audio(&project, &AudioPool::new(), SR).unwrap();
            let mut controller = renderer.controller();
            let before = blocks(&mut renderer, 20, 480, |_| {});
            controller.update(&edited).unwrap();
            let after = blocks(&mut renderer, 10, 480, |_| {});
            let side = |out: &[f32], right: usize| {
                let s: Vec<f64> = out.iter().skip(right).step_by(2).map(|s| f64::from(*s)).collect();
                (s.iter().map(|x| x * x).sum::<f64>() / s.len() as f64).sqrt()
            };
            for (right, want) in [(0, expected.0), (1, expected.1)] {
                let ratio = side(&after, right) / side(&before[4_800 * 2..], right);
                assert!((ratio - want).abs() < 0.01, "{edit:?} side {right}: {ratio}");
            }
        }
    }

    /// Renders `frames` frames from `renderer` in 333-frame blocks.
    fn frames(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames * 2];
        for block in out.chunks_mut(333 * 2) {
            renderer.process(block);
        }
        out
    }

    /// Notes before beat 2 end well before it; notes from beat 2 on.
    fn notes_around_beat_two() -> Project {
        let notes = vec![note(0.0, 0.25, 60), note(1.0, 0.25, 62), note(2.0, 0.5, 64), note(3.25, 0.5, 67)];
        project_with(untargeted(notes), vec![clip(0.0, 4.0)], 1)
    }

    /// A note sounding at the loop end is released there, and the notes of the loop region
    /// start again from the loop start (`docs/spec/arrangement-editing.md` › Loop region).
    #[test]
    fn a_note_over_the_loop_end_is_released_and_the_loop_plays_its_notes_again() {
        let beat = SR as usize / 2;
        let notes = vec![note(0.0, 0.25, 60), note(1.0, 1.5, 62)];
        let mut project = project_with(untargeted(notes), vec![clip(0.0, 4.0)], 1);
        project.transport.loop_range = Some(LoopRange { start: 1.0, end: 2.0 });
        project.transport.loop_enabled = true;
        let mut renderer = Renderer::new(&project, SR).unwrap();
        let out = frames(&mut renderer, 4 * beat);
        // Once the cut note's release has faded, each pass is the first pass again.
        let release = (RELEASE_SECONDS * f64::from(SR)) as usize + 1;
        let first = &out[(beat + release) * 2..2 * beat * 2];
        for pass in 1..3 {
            let at = (beat + pass * beat + release) * 2;
            assert!(max_diff(&out[at..at + first.len()], first) < 1e-6, "pass {pass}");
        }
        assert!(first.iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn playing_from_a_beat_renders_the_tail_of_the_whole_render() {
        let beat = SR as usize / 2;
        let project = notes_around_beat_two();
        let whole = render_offline(&project, SR, 256).unwrap();
        // Running from the start, jumped to beat 2 before the first block.
        let mut renderer = Renderer::new(&project, SR).unwrap();
        renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
        let tail = frames(&mut renderer, whole.len() / 2 - 2 * beat);
        assert!(tail.iter().any(|s| s.abs() > 0.1));
        assert_eq!(max_diff(&tail, &whole[2 * beat * 2..]), 0.0);
        // Stopped, the marker set, then started: the same.
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
        renderer.handle(LaunchCommand::Play);
        let started = frames(&mut renderer, whole.len() / 2 - 2 * beat);
        assert_eq!(max_diff(&started, &whole[2 * beat * 2..]), 0.0);
    }

    #[test]
    fn audio_clips_play_from_their_middle_when_the_transport_starts_inside_them() {
        let beat = SR as usize / 2;
        for mode in [StretchMode::Repitch, StretchMode::Smooth] {
            // 4 s on 8 beats at 120 bpm: the clip runs from beat 0 to 8.
            let (project, pool) = stretched_project(440.0, 4.0, 8.0, 120.0, mode);
            let whole = render_offline_with_audio(&project, &pool, SR, 333).unwrap();
            let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
            renderer.handle(LaunchCommand::SetPosition { beat: 3.0 });
            renderer.handle(LaunchCommand::Play);
            let tail = frames(&mut renderer, whole.len() / 2 - 3 * beat);
            let reference = &whole[3 * beat * 2..];
            assert!(tail.iter().any(|s| s.abs() > 0.1), "{mode:?}");
            let worst = max_diff(&tail, reference);
            // A stretcher started mid-clip has no history yet: close, not identical.
            let allowed = if mode == StretchMode::Repitch { 0.0 } else { 0.05 };
            assert!(worst <= allowed, "{mode:?}: differs by {worst}");
            assert_eq!(renderer.stretch_underruns(), 0, "{mode:?}");
        }
    }

    #[test]
    fn a_jump_while_playing_plays_on_from_the_new_beat() {
        let beat = SR as usize / 2;
        let project = notes_around_beat_two();
        let whole = render_offline(&project, SR, 256).unwrap();
        let mut renderer = Renderer::new(&project, SR).unwrap();
        // Half a beat in (nothing sounding), jump to beat 2.
        let before = frames(&mut renderer, beat / 2);
        assert_eq!(max_diff(&before, &whole[..beat]), 0.0);
        renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
        let after = frames(&mut renderer, whole.len() / 2 - 2 * beat);
        assert_eq!(max_diff(&after, &whole[2 * beat * 2..]), 0.0);
        let monitor = renderer.monitor();
        assert!(monitor.running());
        assert_eq!(monitor.start_beat(), 2.0);
    }

    #[test]
    fn stop_returns_to_the_marker_and_a_second_stop_to_the_start() {
        let project = notes_around_beat_two();
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        let monitor = renderer.monitor();
        renderer.handle(LaunchCommand::SetPosition { beat: 1.5 });
        frames(&mut renderer, 10);
        assert_eq!((monitor.start_beat(), monitor.beat()), (1.5, 1.5), "stopped, the playhead is at the marker");
        renderer.handle(LaunchCommand::Play);
        frames(&mut renderer, SR as usize / 2);
        assert!((monitor.beat() - 2.5).abs() < 1e-3, "{}", monitor.beat());
        renderer.handle(LaunchCommand::Stop);
        frames(&mut renderer, 10);
        assert_eq!((monitor.start_beat(), monitor.beat()), (1.5, 1.5));
        renderer.handle(LaunchCommand::Stop);
        frames(&mut renderer, 10);
        assert_eq!((monitor.start_beat(), monitor.beat()), (0.0, 0.0));
    }

    #[test]
    fn stop_here_moves_the_marker_to_the_playhead() {
        let project = notes_around_beat_two();
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        let monitor = renderer.monitor();
        renderer.handle(LaunchCommand::SetPosition { beat: 1.0 });
        renderer.handle(LaunchCommand::Play);
        frames(&mut renderer, SR as usize);
        renderer.handle(LaunchCommand::StopHere);
        frames(&mut renderer, 10);
        assert!((monitor.start_beat() - 3.0).abs() < 1e-3, "{}", monitor.start_beat());
        // The next start carries on from there.
        renderer.handle(LaunchCommand::Play);
        frames(&mut renderer, 10);
        assert!(monitor.beat() > 3.0 - 1e-3 && monitor.beat() < 3.01, "{}", monitor.beat());
        // Stopped already, it goes to the start like Stop.
        renderer.handle(LaunchCommand::StopHere);
        renderer.handle(LaunchCommand::StopHere);
        frames(&mut renderer, 10);
        assert_eq!(monitor.start_beat(), 0.0);
    }

    #[test]
    fn hostile_positions_are_ignored_or_kept_in_range() {
        let project = notes_around_beat_two();
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        let monitor = renderer.monitor();
        for (beat, want) in [(f64::NAN, 0.0), (-4.0, 0.0), (f64::INFINITY, 0.0), (1e300, MAX_START_BEAT)] {
            renderer.handle(LaunchCommand::SetPosition { beat });
            frames(&mut renderer, 1);
            assert_eq!(monitor.start_beat(), want, "{beat}");
        }
        // Far past the end: plays silence, never panics.
        renderer.handle(LaunchCommand::Play);
        assert!(frames(&mut renderer, 1000).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_metronome_waits_for_the_next_beat_after_a_start_between_beats() {
        let project = project_with(untargeted(Vec::new()), vec![clip(0.0, 4.0)], 1);
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
        renderer.handle(LaunchCommand::SetPosition { beat: 1.5 });
        renderer.handle(LaunchCommand::Play);
        let out = frames(&mut renderer, SR as usize / 2);
        // The first click is at beat 2, a quarter of a second in.
        assert_onsets(&out, &[SR as usize / 4]);
    }

    #[test]
    fn the_cpu_load_is_measured_only_in_real_time() {
        let mut renderer = Renderer::stopped(&Project::default(), &AudioPool::new(), SR).unwrap();
        let controller = renderer.controller();
        let mut block = vec![0.0f32; 2 * 256];
        // Offline renders run flat out: nothing is measured.
        renderer.set_offline(true);
        for _ in 0..100 {
            renderer.process(&mut block);
        }
        assert!(!renderer.cpu.measured_any());
        assert_eq!(controller.cpu_monitor().load_milli(), 0);
        // Real-time playback is measured, and what it measures reaches the controller. The
        // block's own work is tiny, so the test gives the meter known busy times, as an
        // audio thread busy with a big project would have.
        renderer.set_offline(false);
        renderer.process(&mut block);
        assert!(renderer.cpu.measured_any());
        for _ in 0..30 {
            let started = Instant::now();
            renderer.process(&mut block);
            renderer.cpu.measured(started, started + Duration::from_millis(10));
        }
        assert!(controller.cpu_monitor().load_milli() > 200);
    }
}
