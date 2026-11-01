// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina audio engine. Offline rendering and real-time playback share this code path:
//! [`Renderer::new`] does all allocation up front, and [`Renderer::process`] never allocates,
//! so the same `process` call can later run on the audio thread.
//!
//! In detail: Turning a project into audio: note voices, audio clips, real-time time-stretching
//! (ADR 0003), the clip launcher (spec (`docs/spec/clip-launching.md`)), device chains on tracks,
//! with an instrument playing its track's notes (ADR 0004), every beat placed through the project's
//! tempo map in frames (`engine::tempo`; spec (`docs/spec/tempo-map.md`)), plug-in slots in those
//! chains that run a hosted plug-in's audio half (`PluginProcessor`) once per block with its notes,
//! until the block's deadline (ADR 0006), latency compensation across tracks, group and master
//! gain, sends into return tracks' chains, the browser's preview voice after the main mix (ADR
//! 0011), live notes from MIDI controllers played by the armed tracks' instruments, over a queue of
//! their own (`live`, ADR 0017), and the audio input device's frames heard through the tracks whose
//! audio input and monitoring take them (`input`), and the cue bus beside the main output: cued
//! tracks after their own fader, and the preview (cue spec (`docs/spec/cue-output.md`)). Never
//! touches files and never loads a plug-in.

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
mod clock;
mod count_in;
mod cpu;
mod crossfade;
mod cue;
#[cfg(test)]
mod cue_tests;
mod declick;
#[cfg(test)]
mod fade_cost_tests;
#[cfg(test)]
mod fade_tests;
mod follow;
#[cfg(test)]
mod follow_tests;
mod graph;
pub(crate) use graph::Graph;
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
#[cfg(test)]
mod node_tests;
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
mod slot_factory;
mod stretch;
mod swap;
mod tempo;
#[cfg(test)]
mod tempo_tests;
mod track_render;
mod transport;

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
    SongPosition, plugin_devices, plugin_key,
};
pub use plugin_params::{MAX_BLOCK_PARAMS, ParamEvent, RAMP_STEP, param_id};
pub use preview::{PreviewMonitor, PreviewNote, PreviewNotes, preview_notes};
pub use record::{
    RECORD_CHUNK, RECORD_CHUNKS, RECORD_NOTES, RecordReceiver, RecordedAudio, RecordedJump, RecordedNote,
};
use stretch::StretchVoice;
pub use suhina_device_api::NoteEvent as DeviceEvent;
use suhina_device_api::{MusicalScale, ProcessContext};
pub use suhina_model as model;
use suhina_model::{ChannelId, ChannelRole, Clip, ContentLane, LaneData, Project, TempoMap, TrackId};
pub use tempo::frame_of_beat;
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
/// What a block's plug-ins leave of the time its queued frames play ([`Renderer::set_render_lead`]),
/// for the engine itself and the scheduling around it, in seconds.
const LEAD_MARGIN_SECONDS: f64 = 0.003;
/// The device index of a parameter message that moves a track's volume fader
/// ([`Controller::set_device_param`]): the value is the channel's new linear volume, and the
/// playing graph takes it without a new graph where the track's chain can.
pub const MIXER_VOLUME: usize = usize::MAX;
/// The device index of a parameter message that turns a track's send
/// ([`Controller::set_device_param`]): the parameter is the return track's id, the value the
/// send's new linear level; the playing graph takes it without a new graph where the send is
/// in it (it was heard when the graph was built).
pub const MIXER_SEND: usize = usize::MAX - 1;
/// The device index of a parameter message that turns a track's pan
/// ([`Controller::set_device_param`]): the value is the channel's new pan, -1 to 1; the playing
/// graph takes it without a new graph where the track's chain can.
pub const MIXER_PAN: usize = usize::MAX - 2;

/// Whether `project` plays the send from `source` to return `target`: both heard, a level above
/// silence, no loop. A send that plays neither before an edit nor after it changes nothing the
/// engine plays.
pub fn send_heard(project: &Project, source: TrackId, target: TrackId) -> bool {
    sends::plan(project).iter().any(|p| p.source == source && p.target == target)
}

/// Whether `track` is heard in `project`: not muted, nor silenced by a solo, along its way to
/// the main output. The volume and pan of a track that is not heard change nothing it plays.
pub fn track_heard(project: &Project, track: TrackId) -> bool {
    track_gains(project, track).is_some()
}

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
pub const MAX_RENDER_SECONDS: f64 = 60.0 * 60.0;
/// Extra length of a render when tracks have devices, for instrument releases and effect tails.
const CHAIN_TAIL_SECONDS: f64 = 2.0;
/// Most delay latency compensation makes up for, in seconds.
pub(crate) const MAX_LATENCY_SECONDS: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    InvalidProject(Vec<suhina_model::ValidationError>),
    InvalidSampleRate(u32),
    InvalidTempo(f64),
    TooLong {
        seconds: f64,
    },
    /// The source of a node render ([`RenderOptions::source`]) is not a track of the
    /// project with a channel.
    UnknownSource(TrackId),
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
            Self::UnknownSource(track) => write!(
                f,
                "track {} is not a track of the project; the source of a node render is one of its tracks or groups",
                track.0
            ),
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
    /// Its track's chain has a clip host that this clip does not play through: it is added
    /// after the host (#990).
    past_host: bool,
}

/// An ARA clip on a track whose chain has a clip host (#990): the host plays it, and the gains
/// and fades it takes are applied to the host's output from `start` to `end` (arrangement
/// frames).
struct HostedClip {
    start: u64,
    end: u64,
    gain_left: f32,
    gain_right: f32,
    fades: Fades,
    column: u32,
    /// 1 + the index of the track's chain.
    bus: u32,
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
    /// Its key, and whether an arrangement note started it: an edit that takes that note away
    /// ends the voice (#916).
    key: u8,
    arranged: bool,
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
    /// Its track's chain has a clip host that this clip does not play through (#990).
    past_host: bool,
}

/// A source render's capture: the chain whose output the render reads, which chains are inside
/// the source node's subtree (the returns it sends to are not), and the block buffer the
/// source chain's output is mixed into. The offline render copies `buffer` over each block it
/// renders ([`RenderOptions::source`]).
struct GraphCapture {
    chain: usize,
    keep: Vec<bool>,
    buffer: Vec<f32>,
}

/// Plays a project: its arrangement from the start, and launcher clips as they are launched
/// (see [`Renderer::handle`]). Build it once, then call [`Renderer::process`] block by block.
pub struct Renderer {
    sample_rate: u32,
    graph: Box<Graph>,
    /// Where the arrangement plays and whether it rolls (`transport.rs`).
    transport: transport::Transport,
    next_event: usize,
    /// A jump asked for on the playback strip: where it goes and the launcher beat it waits for
    /// (#657).
    pending_jump: Option<(f64, f64)>,
    voices: [Voice; MAX_VOICES],
    next_audio: usize,
    /// Indices into the graph's `audio_events` that are playing; the first `active_audio_count`
    /// are valid.
    active_audio: [usize; MAX_AUDIO_VOICES],
    active_audio_count: usize,
    /// The stretch voices are at their first frame and nothing has played them since.
    stretch_fresh: bool,
    /// A reopened output resumes here after read-ahead, without moving the play start marker.
    start_target: Option<f64>,
    start_guard: declick::StartGuard,
    stretch_scratch: Vec<f32>,
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
    /// The last frame written, and the step a new graph made from it, eased away over
    /// [`DECLICK_FRAMES`] frames so an edit while playing (warp turned on, a clip moved) does not
    /// click where the old graph's sound ends and the new one's begins.
    declick: Declick,
    /// What the device has queued ahead of the next block and how much it takes at once, in
    /// frames ([`Renderer::set_render_lead`]): how long the block's plug-ins may take.
    render_lead: Option<(usize, usize)>,
    #[cfg(test)]
    observed_deadlines: (usize, [Option<Instant>; 2]),
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
    /// The audio thread's CPU load, published to the controller's `CpuMonitor`.
    cpu: cpu::CpuMeter,
    /// The level meters, published to the controller's `MeterMonitor`.
    meters: meters::MeterWriter,
}

/// What a render plays besides the project (#764 › 3): the decoded audio its clips name, the
/// parsed models and IRs its file-backed devices run ([docs/spec/nam-amp-and-cab.md]), and its
/// running plug-ins (ADR 0006). Empty by default: notes and built-in devices only.
#[derive(Default)]
pub struct RenderInputs {
    pub audio: AudioPool,
    pub assets: DeviceAssets,
    pub plugins: Plugins,
}

impl RenderInputs {
    /// Inputs that play the audio clips whose files are in `audio`.
    pub fn from_audio(audio: &AudioPool) -> Self {
        Self { audio: audio.clone(), ..Self::default() }
    }

    /// These inputs, with the file-backed devices running what `assets` holds.
    pub fn with_assets(self, assets: &DeviceAssets) -> Self {
        Self { assets: assets.clone(), ..self }
    }

    /// These inputs, with plug-in devices running in `plugins`.
    pub fn with_plugins(self, plugins: Plugins) -> Self {
        Self { plugins, ..self }
    }
}

/// How a [`Renderer`] starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// The transport is stopped: silent until it receives [`LaunchCommand::Play`] or a launch.
    pub stopped: bool,
    /// One node of the track tree to render instead of the whole mix: a track or a group track,
    /// whose output is the sum at that node, everything that plays into it and its own devices,
    /// and nothing after it (its own volume, pan and mute are in; its groups' and the master's
    /// faders are not). The graph is the one playback builds, with the node a bus for its subtree
    /// and a chain of its own even without devices, so latency compensation is unchanged: a node
    /// render lines up with the whole mix and with the other nodes' renders.
    pub source: Option<TrackId>,
}

impl RenderOptions {
    /// Playing from the start, the whole mix.
    pub const PLAYING: Self = Self { stopped: false, source: None };
    /// With the transport stopped, the whole mix.
    pub const STOPPED: Self = Self { stopped: true, source: None };
}

/// Frames over which the step a new graph makes is eased away (about 1.5 ms at 44.1 kHz).
pub(crate) const DECLICK_FRAMES: usize = 64;
/// How far the new graph's first frame may be from where the old graph's sound was heading
/// before it counts as a step and is eased: sound that goes on through an edit is left exact.
const DECLICK_STEP: f32 = 0.05;

/// Eases away the step between the last frame of one graph and the first of the next.
#[derive(Debug, Default)]
struct Declick {
    /// The last two frames written, the latest first.
    last: [[f32; 2]; 2],
    step: [f32; 2],
    left: usize,
    /// A new graph took over: the next frame written measures the step.
    pending: bool,
}

impl Declick {
    /// Corrects `frames` (interleaved stereo) for a step still being eased, and remembers their
    /// last frames. Never allocates.
    fn ease(&mut self, frames: &mut [f32]) {
        if self.pending && frames.len() >= 2 {
            self.pending = false;
            // Where the old sound was heading: on along its last slope.
            let [latest, before] = self.last;
            let heading = [2.0 * latest[0] - before[0], 2.0 * latest[1] - before[1]];
            let step = [heading[0] - frames[0], heading[1] - frames[1]];
            if step.iter().all(|s| s.is_finite()) && step.iter().any(|s| s.abs() > DECLICK_STEP) {
                self.step = step;
                self.left = DECLICK_FRAMES;
            }
        }
        for frame in frames.as_chunks_mut::<2>().0 {
            if self.left == 0 {
                break;
            }
            let weight = self.left as f32 / DECLICK_FRAMES as f32;
            frame[0] += self.step[0] * weight;
            frame[1] += self.step[1] * weight;
            self.left -= 1;
        }
        match frames {
            [.., l0, r0, l1, r1] => self.last = [[*l1, *r1], [*l0, *r0]],
            [l, r] => self.last = [[*l, *r], self.last[0]],
            _ => {}
        }
    }
}

impl Renderer {
    /// A renderer for the project's notes; audio clips stay silent. See [`Renderer::build`].
    pub fn new(project: &Project, sample_rate: u32) -> Result<Self, RenderError> {
        Self::build(project, RenderInputs::default(), sample_rate, RenderOptions::PLAYING)
    }

    /// A renderer whose transport is stopped, playing the audio clips whose files are in `audio`.
    pub fn stopped(project: &Project, audio: &AudioPool, sample_rate: u32) -> Result<Self, RenderError> {
        Self::build(project, RenderInputs::from_audio(audio), sample_rate, RenderOptions::STOPPED)
    }

    /// A renderer for `project` that plays what `inputs` holds, started as `options` say.
    pub fn build(
        project: &Project,
        inputs: RenderInputs,
        sample_rate: u32,
        options: RenderOptions,
    ) -> Result<Self, RenderError> {
        let RenderInputs { audio, assets, mut plugins } = inputs;
        let mut graph = Graph::build(project, &audio, &assets, sample_rate, &mut plugins, options.source)?;
        if plugins.carried {
            // The old note-off schedule died with its renderer, but the retained
            // instruments did not. Release those voices before chasing notes here.
            for chain in &mut graph.chains {
                chain.release_retained();
            }
        }
        graph.launcher.monitor.running.store(!options.stopped, std::sync::atomic::Ordering::Relaxed);
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
            transport: transport::Transport::new(!options.stopped),
            pending_jump: None,
            voices: [Voice::default(); MAX_VOICES],
            next_audio: 0,
            active_audio: [0; MAX_AUDIO_VOICES],
            active_audio_count: 0,
            stretch_fresh: true,
            start_target: None,
            start_guard: declick::StartGuard::new(sample_rate),
            stretch_scratch: vec![0.0; STRETCH_BLOCK * 2],
            audio: Arc::new(audio),
            assets: Arc::new(assets),
            swap: swap::SwapState::default(),
            underruns_before: 0,
            metronome: metronome::Metronome::new(sample_rate),
            block_frame: 0,
            offline: false,
            declick: Declick::default(),
            render_lead: None,
            #[cfg(test)]
            observed_deadlines: (0, [None; 2]),
            plugin_info: plugins.info,
            preview: preview::Preview::new(sample_rate),
            cue: cue::Cue::new(),
            live: live::Live::new(sample_rate),
            input: input::Input::new(),
            recorder: record::Recorder::new(),
            cpu: cpu::CpuMeter::new(),
            meters,
        })
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
        self.transport.follow_loop = follow;
    }

    /// Ends the renderer and gives back its plug-in devices' processors, with what is known
    /// about each, so another renderer (a new audio output at the same sample rate) can be built
    /// with them ([`Renderer::build`] with [`RenderOptions::STOPPED`]) and the plug-ins keep running. Call it
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
        Plugins { info, processors, carried: true }
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
        if self.transport.running {
            self.transport.position - self.transport.origin + self.transport.time_at_origin
        } else {
            self.start_frame()
        }
    }

    /// Whether the transport is running.
    pub fn running(&self) -> bool {
        self.transport.running
    }

    /// Takes the worker that renders stretched clips ahead, to run it on its own thread
    /// ([`StretchThread::spawn`](crate::ahead::StretchThread::spawn)) so that the audio thread
    /// only copies their frames. Without this the renderer stretches them itself as it goes, as
    /// offline rendering does. `None` once taken.
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
    /// How far the output is ahead of the device as the next block starts: `queued` frames are
    /// waiting to be played before it, and the device takes `device` frames at once. The block's
    /// plug-ins may take the time the complete queued device periods play, less a
    /// margin, instead of a fixed share of the block: a return's plug-ins, which run after every
    /// track that sends to them, missed the share left to them in a large project and crackled.
    /// Call it before every [`Renderer::process`] while a device plays. Never allocates.
    pub fn set_render_lead(&mut self, queued: usize, device: usize) {
        self.render_lead = Some((queued, device));
    }

    pub fn set_live_clock(&mut self, now: u64, delay: u32) {
        self.live.set_clock(now, delay, self.transport.position);
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

    /// The blocks this render's plug-ins did not answer in time (offline, past
    /// [`OFFLINE_TIMEOUT`]): what a mixdown's file would otherwise wait whole timeouts for.
    /// Never allocates.
    pub fn plugin_misses(&self) -> u64 {
        self.graph.launcher.monitor.plugin_misses.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Publishes the plug-in blocks missed so far and the plug-in that missed most. Never
    /// allocates.
    fn publish_plugin_misses(&mut self) {
        let (mut total, mut blocked, mut worst) = (0u64, 0u64, (0u64, 0u64));
        let mut worst_trip = 0u32;
        for chain in &mut self.graph.chains {
            for slot in chain.plugin_slots() {
                total += slot.missed;
                blocked += slot.busy;
                worst_trip = worst_trip.max(slot.longest_us);
                if slot.missed > worst.1 {
                    worst = (slot.key, slot.missed);
                }
            }
        }
        let monitor = &self.graph.launcher.monitor;
        monitor.plugin_misses.store(total, std::sync::atomic::Ordering::Relaxed);
        monitor.plugin_blocked.store(blocked, std::sync::atomic::Ordering::Relaxed);
        monitor.worst_trip.store(worst_trip, std::sync::atomic::Ordering::Relaxed);
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
            let end = self.transport.position + (event.end - time);
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
                arranged: true,
            };
            let Self { voices, graph, transport: crate::transport::Transport { position, .. }, sample_rate, .. } = self;
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
        // The device needs this whole batch at one instant. Splitting it into
        // internal chunks must neither shorten that budget nor renew it.
        let batch_deadline = self.render_lead.and_then(|_| self.deadline(out.len() / 2));
        self.receive_swap();
        self.receive_plugins();
        let misses_before = self.plugin_misses();
        let render_frames = out.len() / 2;
        let mut rest = out;
        let mut cue_rest = cue;
        // Every new graph whose frame falls in this block takes over at its frame: the frames
        // before it are the previous graph's. The queue is bounded, so this ends.
        loop {
            let frames = (rest.len() / 2) as u64;
            match self.swap_at() {
                Some(at) if at < self.transport.position + frames => {
                    let split = ((at.saturating_sub(self.transport.position) * 2) as usize).min(rest.len());
                    let (before, after) = rest.split_at_mut(split);
                    let (cue_before, cue_after) = split_cue(cue_rest, split);
                    if !before.is_empty() {
                        self.process_block(before, cue_before, batch_deadline);
                        self.declick.ease(before);
                    }
                    self.apply_swap();
                    self.declick.pending = true;
                    rest = after;
                    cue_rest = cue_after;
                }
                _ => break,
            }
        }
        if !rest.is_empty() {
            self.process_block(rest, cue_rest, batch_deadline);
            self.declick.ease(rest);
        }
        self.publish_position();
        self.publish_plugin_misses();
        self.publish_count_in();
        if let Some(started) = measured {
            let ended = Instant::now();
            if self.plugin_misses() > misses_before {
                let (queued, period) = self.render_lead.unwrap_or_default();
                self.cpu.monitor.record_late_render(
                    ended.saturating_duration_since(started),
                    queued,
                    period,
                    render_frames,
                );
            }
            self.cpu.measured(started, ended);
        }
    }

    fn process_block(&mut self, mut out: &mut [f32], mut cue: Option<&mut [f32]>, deadline: Option<Instant>) {
        // Device chains hold at most MAX_BLOCK frames; longer blocks are rendered in pieces. A
        // piece also ends at the loop end, where the transport wraps to the loop start, so the
        // wrap lands on its exact frame. Commands are applied first: they can start the
        // transport or move it, which changes where the loop end falls.
        while !out.is_empty() {
            // A jump due at the loop end is made before the wrap, else the wrap moves the
            // playhead to the loop start first and the jump waits for a boundary it has passed.
            self.jump_if_due();
            self.wrap_loop();
            self.drain_commands();
            self.jump_if_due();
            // A count-in that has counted its beats starts the transport on this frame.
            self.end_count_in();
            let frames = (out.len() / 2).min(MAX_BLOCK);
            self.wait_to_start(frames as u64);
            let frames = self.frames_to_loop_end().map_or(frames, |to_end| frames.min(to_end.max(1)));
            let frames = self.frames_to_count_in_end().map_or(frames, |to_end| frames.min(to_end.max(1)));
            // A pending jump lands on its boundary's frame (#657).
            let frames = self.frames_to_jump().map_or(frames, |to_jump| frames.min(to_jump.max(1)));
            let (chunk, rest) = std::mem::take(&mut out).split_at_mut(frames * 2);
            let (cue_chunk, cue_after) = split_cue(cue, chunk.len());
            self.process_chunk(chunk, cue_chunk, deadline);
            // A source render's file audio is the node's captured sum; the master mix the chunk
            // holds first is the whole mix's, which a node render does not read.
            if let Some(capture) = self.graph.capture.as_ref() {
                let end = chunk.len().min(capture.buffer.len());
                if let Some(into) = chunk.get_mut(..end) {
                    into.copy_from_slice(&capture.buffer[..end]);
                }
            }
            cue = cue_after;
            out = rest;
        }
    }

    /// When the plug-ins of a block of `frames` frames starting now must have answered: a share
    /// of the block's playing time, so a late plug-in costs its own sound, not the device's
    /// deadline. `None` offline.
    fn deadline(&self, frames: usize) -> Option<Instant> {
        self.deadline_at(frames, crate::clock::now())
    }

    fn deadline_at(&self, frames: usize, now: Instant) -> Option<Instant> {
        if self.offline {
            return None;
        }
        let rate = f64::from(self.sample_rate.max(1));
        let block = frames as f64 / rate;
        let seconds = match self.render_lead {
            // A complete queued period covers even an immediate callback, leaving
            // until the following callback to supply more. Only a partial period
            // is discounted; subtracting a whole period starves return effects
            // every time a large callback drains the ring to exactly one period.
            // Keep a margin. A partially primed ring still gives half a batch;
            // otherwise use its actual reserve, including for a brief start burst.
            Some((queued, device)) => {
                let period = device.max(1);
                let covered = queued / period * period;
                let room = covered as f64 / rate - LEAD_MARGIN_SECONDS;
                room.max(block * 0.5)
            }
            None => block * DEADLINE_SHARE,
        };
        let duration = Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO);
        Some(now.checked_add(duration).unwrap_or(now))
    }

    fn process_chunk(&mut self, out: &mut [f32], cue: Option<&mut [f32]>, batch_deadline: Option<Instant>) {
        let frames = out.len() / 2;
        self.cue.begin(frames);
        let deadline =
            if self.graph.chains.is_empty() { None } else { batch_deadline.or_else(|| self.deadline(frames)) };
        #[cfg(test)]
        {
            if let Some(observed) = self.observed_deadlines.1.get_mut(self.observed_deadlines.0) {
                *observed = deadline;
            }
            self.observed_deadlines.0 = self.observed_deadlines.0.saturating_add(1);
        }
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
            self.transport.running,
            self.transport.position,
            self.graph.launcher.beat(),
            arrangement_beat,
            beats_per_frame,
            self.live.delay(),
            self.input.waiting(),
        );
        // Live notes after the commands, so a track armed in this block hears them.
        self.live.receive(&mut self.graph.chains, &mut self.recorder, self.transport.position);
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
            playing: self.transport.running,
        };
        // Plug-in parameter events (set values, automation) are planned before any of the
        // block runs: a chain may run part of it early, when notes crowd it.
        let at = self.transport.running.then(|| self.time());
        let time = self.time();
        let (bar, bar_start, signature) = self.graph.launcher.bar();
        // The loop is an active cycle only while playback repeats it: a render that plays
        // straight through it tells the plug-ins of no cycle.
        let cycle = self.graph.loop_beats.filter(|_| self.transport.follow_loop);
        let song = plugin::SongPosition {
            playing: self.transport.running,
            position: time,
            tempo: context.tempo,
            beat: context.beat,
            bar,
            bar_start,
            numerator: signature.numerator,
            denominator: signature.denominator,
            cycle,
            cycle_seconds: cycle.map(|(start, end)| {
                let seconds = |beat| self.frame_of_beat(beat) as f64 / f64::from(self.sample_rate.max(1));
                (seconds(start), seconds(end))
            }),
        };
        for chain in &mut self.graph.chains {
            chain.transport(&song);
            chain.automate(at, frames);
            chain.automate_mix(time, frames, self.transport.running);
        }
        let attack = (ATTACK_SECONDS * f64::from(self.sample_rate)).max(1.0);
        let release = (RELEASE_SECONDS * f64::from(self.sample_rate)).max(1.0);
        let block_start = self.transport.position;
        // The transport changes only through commands, which are applied above.
        let running = self.transport.running;
        for (frame, [out_left, out_right]) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            self.block_frame = frame;
            for chain in self.graph.chains.iter_mut().filter(|c| c.instrument) {
                chain.release_due(frame, self.transport.position);
            }
            self.preview.frame(&mut self.graph.chains, frame, self.transport.position);
            self.live.frame(&mut self.graph.chains, &mut self.recorder, frame, self.transport.position);
            // The click for this frame's beat, read before the launcher moves the beat on.
            let launcher = &self.graph.launcher;
            let click =
                self.metronome.frame(self.transport.running, launcher.beat(), |index| launcher.starts_bar(index))
                    + self.count_in_frame();
            let (launched_left, launched_right) = self.launcher_frame();
            let time = self.time();
            if self.transport.running && self.transport.chase {
                self.transport.chase = false;
                self.chase_notes(frame, time);
            }
            while self.transport.running
                && let Some(event) = self.graph.events.get(self.next_event)
            {
                if event.start > time {
                    break;
                }
                let event = *event;
                self.next_event += 1;
                if !self.graph.launcher.overrides(event.column) {
                    let end = self.transport.position + event.end.saturating_sub(event.start);
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
                        arranged: true,
                    };
                    let Self {
                        voices,
                        graph,
                        transport: crate::transport::Transport { position, .. },
                        sample_rate,
                        ..
                    } = self;
                    start_note(voices, &mut graph.chains, frame, *position, *sample_rate, end, start);
                }
            }

            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for voice in self.voices.iter_mut().filter(|v| v.active) {
                let age = (self.transport.position - voice.started_at) as f64;
                let envelope = if self.transport.position < voice.end {
                    (age / attack).min(1.0)
                } else {
                    let released = (self.transport.position - voice.end) as f64;
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
            let (audio_left, audio_right) =
                if self.transport.running { self.audio_frame(time, frame) } else { (0.0, 0.0) };
            *out_left = left + audio_left + launched_left + click;
            *out_right = right + audio_right + launched_right + click;
            self.transport.position += 1;
        }
        if running {
            self.mix_stretched(out, block_start);
            self.host_gains(frames, block_start);
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
        // With a source render's capture, the source chain's output is mixed into the capture
        // buffer instead of where it plays.
        let capture = self.graph.capture.as_mut().map(|capture| sends::Capture {
            chain: capture.chain,
            keep: &capture.keep,
            buffer: &mut capture.buffer,
        });
        sends::finish(&mut self.graph.chains, out, &mut self.cue.bus, frames, &context, capture);
        self.graph.levels.finish(&self.graph.chains, frames, &mut self.meters);
        // The preview comes after every track and the main channel's mix (ADR 0011), into the
        // cue bus, and into the main output too while the cue plays with it.
        if let Some(preview) = self.cue.preview.get_mut(..frames * 2) {
            self.preview.finish(preview, frames, &mut self.graph.chains);
        }
        self.cue.finish(out, frames);
        if let Some(bus) = self.cue.bus.get_mut(..frames * 2) {
            self.start_guard.process(out, bus);
        }
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

    /// Sets the gains the clip hosts' output takes over the block of `frames` frames starting at
    /// frame `block_start`: each playing ARA clip's gain and fades (#990). Never allocates.
    fn host_gains(&mut self, frames: usize, block_start: u64) {
        let time_start = block_start - self.transport.origin + self.transport.time_at_origin;
        let time_end = time_start + frames as u64;
        let launcher = &self.graph.launcher;
        let chains = &mut self.graph.chains;
        for clip in &self.graph.hosted {
            if clip.start >= time_end {
                break;
            }
            if clip.end <= time_start {
                continue;
            }
            let Some(chain) = bus_chain(chains, clip.bus) else { continue };
            for frame in clip.start.max(time_start)..clip.end.min(time_end) {
                let at = (frame - time_start) as usize;
                let (left, right) = if launcher.overrides_at(clip.column, frame - time_start + block_start) {
                    (0.0, 0.0)
                } else {
                    let fade = clip.fades.gain((frame - clip.start) as f64, (clip.end - frame) as f64);
                    (clip.gain_left * fade, clip.gain_right * fade)
                };
                chain.set_host_gain(at, left, right);
            }
        }
    }

    /// Adds the stretched clips that overlap the block starting at frame `block_start` to `out`.
    /// Clips on a track that plays launched clips keep running, so they stay in time, but are
    /// not heard.
    fn mix_stretched(&mut self, out: &mut [f32], block_start: u64) {
        // Arrangement time of the block's first frame.
        let time_start = block_start - self.transport.origin + self.transport.time_at_origin;
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
                        if event.past_host {
                            chain.add_past_host(at, l, r);
                        } else {
                            chain.add(at, l, r);
                        }
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
                            Some(chain) if arranged.past_host => chain.add_past_host(frame, l, r),
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
    /// Whether it is an arrangement note, which an edit can take away while it sounds.
    pub(crate) arranged: bool,
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
        if note.arranged {
            chain.mark_arranged(note.key);
        }
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
            key: note.key,
            arranged: note.arranged,
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
/// Audio clips stay silent; see [`render_offline_from`].
pub fn render_offline(project: &Project, sample_rate: u32, block_frames: usize) -> Result<Vec<f32>, RenderError> {
    render_offline_from(project, RenderInputs::default(), sample_rate, block_frames, None)
}

/// Renders the whole arrangement offline into interleaved stereo, `block_frames` at a time,
/// playing what `inputs` holds: the same `process` as playback, waiting for each plug-in instead
/// of a real-time deadline. With `source`, the mix at that node of the track tree
/// ([`RenderOptions::source`]); the same length as the whole mix's render, so renders line up.
pub fn render_offline_from(
    project: &Project,
    inputs: RenderInputs,
    sample_rate: u32,
    block_frames: usize,
    source: Option<TrackId>,
) -> Result<Vec<f32>, RenderError> {
    let mut renderer =
        Renderer::build(project, inputs, sample_rate, RenderOptions { source, ..RenderOptions::PLAYING })?;
    renderer.set_offline(true);
    // The whole arrangement, once: a render does not repeat the loop region.
    renderer.set_follow_loop(false);
    let mut out = vec![0.0f32; offline_length(&renderer)?];
    for block in out.chunks_mut(block_frames.max(1).saturating_mul(2)) {
        renderer.process(block);
    }
    Ok(out)
}

/// Samples (interleaved stereo) of a render of the whole arrangement.
fn offline_length(renderer: &Renderer) -> Result<usize, RenderError> {
    usize::try_from(renderer.length())
        .ok()
        .and_then(|frames| frames.checked_mul(2))
        .ok_or(RenderError::TooLong { seconds: renderer.length() as f64 / f64::from(renderer.sample_rate()) })
}

/// Renders the whole arrangement offline like [`render_offline_from`], and the cue bus beside
/// it: the main mix and the cue, both interleaved stereo of the same length. `cue_gain` is the
/// cue volume (linear) and `separate` whether the cue has outputs of its own (then the preview is
/// heard only on the cue, as on the device).
pub fn render_offline_with_cue(
    project: &Project,
    inputs: RenderInputs,
    sample_rate: u32,
    block_frames: usize,
    cue_gain: f32,
    separate: bool,
) -> Result<(Vec<f32>, Vec<f32>), RenderError> {
    let mut renderer = Renderer::build(project, inputs, sample_rate, RenderOptions::PLAYING)?;
    renderer.set_offline(true);
    renderer.handle(LaunchCommand::SetCue { gain: Some(cue_gain), separate: Some(separate) });
    let total = offline_length(&renderer)?;
    let mut out = vec![0.0f32; total];
    let mut cue = vec![0.0f32; total];
    let block = block_frames.max(1).saturating_mul(2);
    for (main, cued) in out.chunks_mut(block).zip(cue.chunks_mut(block)) {
        renderer.process_with_cue(main, cued);
    }
    Ok((out, cue))
}

/// Renders `seconds` of the project offline, starting with the transport stopped and applying
/// each launch command at its time in seconds (in order). Audio clips play from `audio`; see
/// [`render_launches_from`] for plug-ins.
pub fn render_launches(
    project: &Project,
    audio: &AudioPool,
    sample_rate: u32,
    commands: &[(f64, LaunchCommand)],
    seconds: f64,
) -> Result<Vec<f32>, RenderError> {
    render_launches_from(project, RenderInputs::from_audio(audio), sample_rate, commands, seconds)
}

/// [`render_launches`] playing what `inputs` holds. This is the same path a live
/// [`Controller`] drives, so launcher behaviour renders offline exactly as it plays.
pub fn render_launches_from(
    project: &Project,
    inputs: RenderInputs,
    sample_rate: u32,
    commands: &[(f64, LaunchCommand)],
    seconds: f64,
) -> Result<Vec<f32>, RenderError> {
    if !(seconds.is_finite() && seconds >= 0.0) || seconds > MAX_RENDER_SECONDS {
        return Err(RenderError::TooLong { seconds });
    }
    let mut renderer = Renderer::build(project, inputs, sample_rate, RenderOptions::STOPPED)?;
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
    let channel_id = project.track(track)?.channel?;
    let channel = project.channel(channel_id)?;
    let pan = if channel.pan.is_finite() { channel.pan.clamp(-1.0, 1.0) } else { 0.0 };
    let gain = |c: &suhina_model::Channel| if c.volume.is_finite() { c.volume.max(0.0) } else { 0.0 };

    let mut volume = 1.0;
    let mut visited = Vec::new();
    let mut next = Some(channel);
    // A main channel with a chain of its own applies its volume and mute after it (#687), also
    // for a track routed to it by name.
    let main_chain = buses::main_runs_chain(project);
    // Bounded, so a routing cycle in a malformed project cannot loop forever.
    while let Some(c) = next.take() {
        if main_chain && c.role == ChannelRole::Master {
            break;
        }
        // A solo outranks a mute: a muted track soloed is heard (its muted clips are not).
        if (c.mute && !c.solo) || visited.len() > 64 || visited.contains(&c.id) {
            return None;
        }
        visited.push(c.id);
        volume *= gain(c);
        next = c.output.and_then(|id| project.channel(id));
    }
    if !heard_with_solo(project, channel, &visited) {
        return None;
    }
    // The main channel's volume and mute apply after its chain when it has one (#687).
    if !buses::main_runs_chain(project)
        && let Some(master) =
            project.channels.iter().find(|c| c.role == ChannelRole::Master && !visited.contains(&c.id))
    {
        if master.mute {
            return None;
        }
        volume *= gain(master);
    }
    let (left, right) = suhina_dsp::pan::gains(pan);
    // Scaled so a centred note has unity gain on each side, and quieter overall to leave headroom.
    let scale = volume * std::f64::consts::SQRT_2 * 0.25;
    Some(((left * scale) as f32, (right * scale) as f32))
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
                Some(id) => next = project.channel(id).and_then(|o| o.output),
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
#[path = "lib_tests.rs"]
mod tests;
