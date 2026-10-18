// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plug-ins a project runs (ADR 0006): for each plug-in device, the installed plug-in found
//! for it in the catalog, started in a host process with the state the project saved, off the
//! audio thread, and handed to the playing engine; its health watched, and a restart that gets
//! its last state back.
//!
//! The engine only sees each plug-in's audio half ([`Hosted`], a [`PluginProcessor`]); the
//! control half ([`Plugin`]) stays here, on the session's thread. A plug-in that is not
//! installed is *missing*: the device stays in the project with its state untouched, and the
//! engine passes the sound through it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use suhina_engine::{
    DeviceEvent, Followed, Joint, MAX_BLOCK_FRAMES, MAX_BLOCK_PARAMS, ParamEvent, PluginProcessor, PluginSlotInfo,
    Plugins, Sent, model::DeviceKind, plugin_devices,
};
use suhina_model::{Device, Project};
use suhina_plugins::library::{Hosting, group_for};
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_plugins::{
    Event, EventKind, Health, Launch, MAX_EVENTS, Placement, Plugin, PluginAudio, PluginError, SavedState,
};

use crate::plugin_edits::PluginEdits;
use crate::{CommandError, Player};

/// How long starting a plug-in (the host process, loading it, its state) may take. Generous:
/// a plug-in that reads a large sample library as it loads can take half a minute or more
/// without being hung, and the loading dialog shows which plug-ins are still at it.
pub const START_TIMEOUT: Duration = Duration::from_secs(120);
/// How often the session looks whether its plug-ins still run.
const HEALTH_EVERY: Duration = Duration::from_millis(250);
/// How often the state of each running plug-in is read, so a restart after a crash gets back
/// what the user set in its editor (ADR 0006, decision 5).
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// How long saving a project waits for its plug-ins to give their state.
const SAVE_WAIT: Duration = Duration::from_secs(1);
/// MIDI controller numbers an all-notes-off is sent as.
const ALL_NOTES_OFF: u8 = 123;
const SUSTAIN: u8 = 64;

/// The host process group a plug-in runs in (`None`: a process of its own).
pub type ProcessGroup = Box<dyn Fn(&PluginInfo) -> Option<String> + Send + Sync>;

/// What the session needs to start plug-ins: the catalog of installed ones, the host
/// executable, and which plug-ins share a host process (ADR 0006, decision 2).
pub struct PluginEnv {
    pub catalog: Catalog,
    pub host: PathBuf,
    /// The process group of a plug-in (`None`: a process of its own).
    pub group: ProcessGroup,
}

impl PluginEnv {
    /// `catalog` with plug-ins run by `host`, every instance of one plug-in file in one process
    /// (the default).
    pub fn new(catalog: Catalog, host: PathBuf) -> Self {
        Self { catalog, host, group: Box::new(|plugin| group_for(Hosting::default(), plugin)) }
    }

    /// What `library` gives: its catalog, its host, and how each plug-in is hosted (its
    /// `plugins.setHosting` choice, else one process per plug-in file).
    pub fn from_library(library: &mut suhina_plugins::library::Library) -> Self {
        let host = library.host().to_path_buf();
        let catalog = library.catalog().clone();
        let chosen: HashMap<String, Hosting> = catalog
            .plugins()
            .map(PluginInfo::spec)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|spec| {
                let hosting = library.hosting(&spec);
                (spec, hosting)
            })
            .filter(|(_, hosting)| *hosting != Hosting::default())
            .collect();
        let group: ProcessGroup =
            Box::new(move |plugin| group_for(chosen.get(&plugin.spec()).copied().unwrap_or_default(), plugin));
        Self { catalog, host, group }
    }
}

/// Where a plug-in device stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Its host process is starting it.
    Loading,
    Running,
    /// No installed plug-in stands for it; it keeps its state and passes the sound through.
    Missing,
    /// Its host process exited or stopped answering; the engine plays on without it until it
    /// is restarted.
    Crashed(String),
    /// It could not be started.
    Failed(String),
}

impl PluginStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Running => "running",
            Self::Missing => "missing",
            Self::Crashed(_) => "crashed",
            Self::Failed(_) => "failed",
        }
    }

    /// Why it crashed or failed.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Crashed(why) | Self::Failed(why) => Some(why),
            _ => None,
        }
    }
}

/// One plug-in device's running plug-in, as the session sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginReport {
    pub status: PluginStatus,
    /// The installed plug-in that stands for it: name, vendor, how it was found.
    pub plugin: Option<(String, String, &'static str)>,
    pub instrument: bool,
    /// Frames of delay it reported.
    pub latency: u32,
    pub editor_open: bool,
    /// The open editor is embedded in the device panel of the session's window (rather than
    /// in a window of its own), and the size of its inside in pixels, as the host last said.
    pub editor_embedded: Option<(u32, u32)>,
    /// Blocks the engine gave it since it started, and how many it did not answer in time
    /// (the chain went on without it for that block).
    pub blocks: u32,
    pub late_blocks: u32,
    /// The longest a block's round trip took, in microseconds.
    pub longest_us: u32,
    /// Blocks that came with no time left before the deadline (an earlier plug-in used it).
    pub no_time: u32,
    /// Why it runs with its own defaults, when it did not take the saved state.
    pub state_refused: Option<String>,
}

/// How far a project's plug-ins have got with starting, for a dialog while they load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginProgress {
    /// Plug-ins that are installed and should run.
    pub total: usize,
    pub ready: usize,
    /// The names of those still starting.
    pub loading: Vec<String>,
    /// Plug-in devices with no installed plug-in.
    pub missing: usize,
    /// The names of those that could not start or crashed.
    pub failed: Vec<String>,
}

/// How a hosted plug-in keeps up with the engine: the blocks it was given and the ones it did
/// not answer in time (sent late, or not sent because the host was still busy), counted on
/// the audio thread and read by the session for the device panel.
#[derive(Debug, Default)]
pub struct BlockCounts {
    blocks: AtomicU32,
    late: AtomicU32,
    /// The longest a block's round trip took, in microseconds.
    longest_us: AtomicU32,
    /// Blocks that came with no time left before the deadline (an earlier plug-in used it).
    no_time: AtomicU32,
}

impl BlockCounts {
    /// Blocks given and blocks not answered in time, so far.
    pub fn read(&self) -> (u32, u32) {
        (self.blocks.load(Ordering::Relaxed), self.late.load(Ordering::Relaxed))
    }

    /// The longest a block's round trip took, in microseconds.
    pub fn longest_us(&self) -> u32 {
        self.longest_us.load(Ordering::Relaxed)
    }

    /// Blocks that came with no time left before the deadline.
    pub fn no_time(&self) -> u32 {
        self.no_time.load(Ordering::Relaxed)
    }
}

/// The audio half of a hosted plug-in, as the engine runs it: real-time safe.
pub struct Hosted {
    audio: PluginAudio,
    input: Vec<f32>,
    output: Vec<f32>,
    /// The block's events as sent: parameter values and notes merged in frame order.
    events: Vec<Event>,
    /// The block's notes and its parameter values, before they are merged.
    notes: Vec<Event>,
    params: Vec<Event>,
    /// The parameter values the engine gave for the next block ([`PluginProcessor::automate`]).
    automated: Vec<Event>,
    /// Releases and parameter values (the latest of each) from blocks that were not sent (the
    /// host still busy with an earlier one), sent at the start of the next block that is, so
    /// no note is left hanging and no automation is lost.
    carried: Vec<Event>,
    counts: Arc<BlockCounts>,
    /// When the block out was sent, for `longest_us`: the engine may look at other chains'
    /// plug-ins before it waits for this one's answer (#151), so the round trip is timed from
    /// here, not from the wait.
    sent_at: Option<std::time::Instant>,
}

impl Hosted {
    pub fn new(audio: PluginAudio) -> Self {
        Self {
            audio,
            input: vec![0.0; MAX_BLOCK_FRAMES * 2],
            output: vec![0.0; MAX_BLOCK_FRAMES * 2],
            events: Vec::with_capacity(MAX_EVENTS),
            notes: Vec::with_capacity(MAX_EVENTS),
            params: Vec::with_capacity(MAX_EVENTS),
            automated: Vec::with_capacity(MAX_BLOCK_PARAMS),
            carried: Vec::with_capacity(MAX_EVENTS),
            counts: Arc::new(BlockCounts::default()),
            sent_at: None,
        }
    }

    /// The counts this keeps while the engine runs it.
    pub fn counts(&self) -> Arc<BlockCounts> {
        Arc::clone(&self.counts)
    }
}

/// Keeps the releases among `events` in `carried`, and the latest value of each parameter,
/// within its capacity. Never allocates.
fn carry_releases(events: &[Event], carried: &mut Vec<Event>) {
    let room = carried.capacity();
    for event in events.iter().filter(|e| releases(e) || matches!(e.kind, EventKind::Param { .. })) {
        if let EventKind::Param { id, .. } = event.kind
            && let Some(older) =
                carried.iter_mut().find(|c| matches!(c.kind, EventKind::Param { id: other, .. } if other == id))
        {
            *older = *event;
            continue;
        }
        if carried.len() < room {
            carried.push(*event);
        }
    }
}

/// `params` and `notes`, each in frame order, into `out` in frame order, a parameter value
/// before a note at the same frame (so a note starts with the sound it is set to), within
/// `out`'s capacity. Never allocates.
fn merge(params: &[Event], notes: &[Event], out: &mut Vec<Event>) {
    let room = out.capacity().min(MAX_EVENTS);
    let (mut params, mut notes) = (params.iter().peekable(), notes.iter().peekable());
    while out.len() < room {
        let next = match (params.peek(), notes.peek()) {
            (Some(p), Some(n)) if p.frame <= n.frame => params.next(),
            (_, Some(_)) => notes.next(),
            (Some(_), None) => params.next(),
            (None, None) => break,
        };
        if let Some(event) = next {
            out.push(*event);
        }
    }
}

/// Whether `event` lets go of something (a note, the sustain pedal, all notes), so it must
/// not be lost with a block that was not sent.
fn releases(event: &Event) -> bool {
    match event.kind {
        EventKind::NoteOff { .. } => true,
        EventKind::Controller { number, .. } => number == SUSTAIN || number == ALL_NOTES_OFF,
        _ => false,
    }
}

/// `event` at `frame` as the plug-in host takes it, into `out` (within its capacity).
fn push_event(out: &mut Vec<Event>, frame: u32, event: DeviceEvent) {
    let mut push = |kind: EventKind| {
        if out.len() < out.capacity().min(MAX_EVENTS) {
            out.push(Event { frame, note: -1, kind });
        }
    };
    match event {
        DeviceEvent::On { key, velocity } if key < 128 => {
            push(EventKind::NoteOn { channel: 0, key, velocity: velocity.clamp(0.0, 1.0) });
        }
        DeviceEvent::Off { key, velocity } if key < 128 => {
            push(EventKind::NoteOff { channel: 0, key, velocity: velocity.clamp(0.0, 1.0) });
        }
        DeviceEvent::Controller { number, value } if number < 128 => {
            push(EventKind::Controller { channel: 0, number, value: value.clamp(0.0, 1.0) });
        }
        DeviceEvent::PitchBend(value) => push(EventKind::PitchBend { channel: 0, value: value.clamp(-1.0, 1.0) }),
        DeviceEvent::AllNotesOff => {
            push(EventKind::Controller { channel: 0, number: SUSTAIN, value: 0.0 });
            push(EventKind::Controller { channel: 0, number: ALL_NOTES_OFF, value: 0.0 });
        }
        DeviceEvent::On { .. }
        | DeviceEvent::Off { .. }
        | DeviceEvent::Controller { .. }
        | DeviceEvent::Pressure(_) => {}
    }
}

impl Hosted {
    /// Copies the block and its events in, ready to send; `false` for a block too big.
    fn take(&mut self, buffer: &[f32], events: &[(usize, DeviceEvent)]) -> bool {
        let Some(input) = self.input.get_mut(..buffer.len()) else { return false };
        input.copy_from_slice(buffer);
        self.notes.clear();
        self.params.clear();
        // What earlier blocks could not send goes first.
        for event in self.carried.drain(..) {
            let event = Event { frame: 0, ..event };
            match event.kind {
                EventKind::Param { .. } => self.params.push(event),
                _ => self.notes.push(event),
            }
        }
        for &(frame, event) in events {
            push_event(&mut self.notes, u32::try_from(frame).unwrap_or(u32::MAX), event);
        }
        let room = self.params.capacity().saturating_sub(self.params.len());
        let automated = self.automated.len().min(room);
        self.params.extend(self.automated.drain(..automated));
        self.automated.clear();
        self.events.clear();
        merge(&self.params, &self.notes, &mut self.events);
        self.counts.blocks.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Takes in what a block's round trip gave: the output into `buffer` when it was answered.
    fn outcome(&mut self, outcome: suhina_plugins::Outcome, buffer: &mut [f32]) -> bool {
        match outcome {
            suhina_plugins::Outcome::Done => {
                if let Some(output) = self.output.get(..buffer.len()) {
                    buffer.copy_from_slice(output);
                }
                true
            }
            // Sent, but late: the host still gets its events.
            suhina_plugins::Outcome::Missed => {
                self.counts.late.fetch_add(1, Ordering::Relaxed);
                false
            }
            // Not sent: its releases wait for the next block; new notes are dropped.
            suhina_plugins::Outcome::Busy => {
                self.counts.late.fetch_add(1, Ordering::Relaxed);
                carry_releases(&self.events, &mut self.carried);
                false
            }
        }
    }

    /// Waits for the block out, noting how long it took and whether any time was left.
    fn wait_for(&mut self, buffer: &mut [f32], timeout: Duration) -> bool {
        if timeout.is_zero() {
            self.counts.no_time.fetch_add(1, Ordering::Relaxed);
        }
        let started = self.sent_at.take().unwrap_or_else(std::time::Instant::now);
        let outcome = match self.output.get_mut(..buffer.len()) {
            Some(output) => self.audio.finish(output, timeout),
            None => suhina_plugins::Outcome::Busy,
        };
        let took = u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX);
        self.counts.longest_us.fetch_max(took, Ordering::Relaxed);
        self.outcome(outcome, buffer)
    }
}

impl PluginProcessor for Hosted {
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, DeviceEvent)], timeout: Duration) -> bool {
        if self.send(buffer, events, timeout) != Sent::Out {
            return false;
        }
        self.wait_for(buffer, timeout)
    }

    /// Never [`Sent::Unsupported`]: a block that cannot go (the host still busy with an earlier
    /// block after `patience`) is refused, so the chain must not run it again (#188). It counts
    /// once as a late block here and in the engine; when it was the host that was busy, once as
    /// unanswered too (`PluginAudio::send`).
    fn send(&mut self, buffer: &[f32], events: &[(usize, DeviceEvent)], patience: Duration) -> Sent {
        self.send_joint(buffer, events, &[], patience)
    }

    fn wait(&mut self, buffer: &mut [f32], timeout: Duration) -> bool {
        self.wait_for(buffer, timeout)
    }

    fn poll(&mut self, until: std::time::Instant) -> bool {
        self.audio.poll(until)
    }

    fn ready(&mut self, until: std::time::Instant) -> bool {
        self.audio.ready(until)
    }

    /// Its host process and number there, unless releases from a block it could not take wait
    /// to be sent (they go on its own link first) or its link still has a block out.
    fn joint(&self) -> Option<Joint> {
        if !self.carried.is_empty() {
            return None;
        }
        self.audio.seat().map(|seat| Joint { process: seat.process, member: seat.instance })
    }

    /// As [`Hosted::send`]; the followers run in the same host process (ADR 0006, decision 2).
    fn send_joint(
        &mut self,
        buffer: &[f32],
        events: &[(usize, DeviceEvent)],
        followers: &[u32],
        patience: Duration,
    ) -> Sent {
        // A block longer than the link holds (none is, today) cannot be taken in at all.
        let Some(input) = self.take(buffer, events).then(|| self.input.get(..buffer.len())).flatten() else {
            self.counts.late.fetch_add(1, Ordering::Relaxed);
            return Sent::Refused;
        };
        if self.audio.send_joint(input, &self.events, followers, buffer.len(), patience) {
            self.sent_at = Some(std::time::Instant::now());
            return Sent::Out;
        }
        // The host is still busy with an earlier block: this one is lost to it, as a miss; its
        // note releases go with the next block.
        self.counts.late.fetch_add(1, Ordering::Relaxed);
        carry_releases(&self.events, &mut self.carried);
        Sent::Refused
    }

    /// Counts a block it followed in like one of its own: given, late unless answered, and
    /// a refused one's releases kept for its next block.
    fn followed(&mut self, events: &[(usize, DeviceEvent)], how: Followed) {
        self.counts.blocks.fetch_add(1, Ordering::Relaxed);
        let outcome = match how {
            Followed::Answered => suhina_plugins::Outcome::Done,
            Followed::Late => suhina_plugins::Outcome::Missed,
            Followed::Refused => suhina_plugins::Outcome::Busy,
        };
        self.audio.followed(outcome);
        if how == Followed::Answered {
            return;
        }
        self.counts.late.fetch_add(1, Ordering::Relaxed);
        if how == Followed::Refused {
            self.events.clear();
            for &(frame, event) in events {
                push_event(&mut self.events, u32::try_from(frame).unwrap_or(u32::MAX), event);
            }
            carry_releases(&self.events, &mut self.carried);
        }
    }

    fn automate(&mut self, params: &[ParamEvent]) {
        self.automated.clear();
        let room = self.automated.capacity();
        for param in params.iter().take(room) {
            let frame = u32::try_from(param.frame).unwrap_or(u32::MAX);
            // Kept finite: the host hands no plug-in a value that is not a number.
            let value = if param.value.is_finite() {
                param.value.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32
            } else {
                0.0
            };
            self.automated.push(Event::param(frame, param.id, value));
        }
    }
}

/// The installed plug-in for `device` and whether it is an instrument; `None` when nothing
/// installed stands for it (or it is not a plug-in).
fn find<'a>(catalog: &'a Catalog, device: &Device) -> Option<(suhina_plugins::matching::Match<'a>, bool)> {
    let (found, _) = crate::plugins::device_plugin(catalog, device)?;
    let instrument = is_instrument(found.plugin);
    Some((found, instrument))
}

/// Whether the plug-in plays notes, from the features its scan found (CLAP `instrument`, VST3
/// subcategories such as `Instrument|Synth`).
pub fn is_instrument(plugin: &PluginInfo) -> bool {
    plugin.features.iter().any(|f| f.to_ascii_lowercase().contains("instrument"))
}

/// Suhina's group for the plug-in, from the format's own sub-categories, as the browser's
/// group-by-type view shows it ([the plug-in browsing spec](../docs/spec/plugin-browsing.md)):
/// `None` is the type's **Other**. The first of the spec's tables that matches wins, so a
/// VST3's `Fx|Dynamics|Compressor` is Dynamics, not one group per word.
pub fn plugin_group(plugin: &PluginInfo) -> Option<&'static str> {
    // The words one format or the other uses for each group, lower-cased, whole words.
    const GROUPS: [(&str, &[&str]); 12] = [
        ("Synth", &["synth", "synthesizer"]),
        ("Sampler", &["sampler"]),
        ("Drums", &["drum", "drum-machine"]),
        ("Reverb", &["reverb"]),
        ("Delay", &["delay"]),
        ("EQ", &["eq"]),
        ("Filter", &["filter"]),
        ("Dynamics", &["dynamics", "compressor", "limiter", "gate"]),
        ("Distortion", &["distortion", "saturation"]),
        ("Modulation", &["modulator", "chorus", "flanger", "phaser", "tremolo", "vibrato"]),
        ("Analyzer", &["analyzer"]),
        ("Utility", &["utility", "spatial"]),
    ];
    // The features arrive as separate words already (a VST3's sub-categories are split on
    // their `|` in the scan, a CLAP's features are one word each); they are matched whole,
    // case-insensitively. The type words ("Fx", "Instrument", "audio-effect") name no group.
    let features: Vec<String> = plugin.features.iter().map(|f| f.to_ascii_lowercase()).collect();
    GROUPS.iter().find_map(|(group, words)| features.iter().any(|f| words.contains(&f.as_str())).then_some(*group))
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn plugin(features: &[&str]) -> PluginInfo {
        PluginInfo {
            path: PathBuf::from("/vst3/Thing.vst3"),
            id: "00000000000000000000000000000000".into(),
            name: "Thing".into(),
            vendor: "Maker".into(),
            version: "1".into(),
            features: features.iter().map(|f| (*f).into()).collect(),
            replaces: Vec::new(),
        }
    }

    #[test]
    fn a_plugins_group_comes_from_its_formats_categories() {
        // A VST3's sub-categories, as the scan splits them; the first group in the table wins.
        assert_eq!(plugin_group(&plugin(&["Fx", "Dynamics", "Compressor"])), Some("Dynamics"));
        assert_eq!(plugin_group(&plugin(&["Instrument", "Synth"])), Some("Synth"));
        // Case-insensitive, whole words: "Spectrum" is no group, so Other.
        assert_eq!(plugin_group(&plugin(&["Fx", "Spectrum"])), None);
        assert_eq!(plugin_group(&plugin(&["ANALYZER"])), Some("Analyzer"));
        // A CLAP's features.
        assert_eq!(plugin_group(&plugin(&["audio-effect", "reverb"])), Some("Reverb"));
        assert_eq!(plugin_group(&plugin(&["instrument", "synthesizer"])), Some("Synth"));
        assert_eq!(plugin_group(&plugin(&["instrument", "drum-machine"])), Some("Drums"));
        // The type words name no group; a plug-in with none is Other.
        assert_eq!(plugin_group(&plugin(&["audio-effect"])), None);
        assert_eq!(plugin_group(&plugin(&["Instrument", "External"])), None);
        assert_eq!(plugin_group(&plugin(&[])), None);
    }
}

/// The state the project saved for `device`, with a VST3 controller's own state.
fn saved_state(device: &Device) -> Option<SavedState> {
    let controller = device.extensions.0.iter().find(|e| e.key == "Vst3ControllerState").map(|e| e.data.clone());
    device.state.clone().map(|data| SavedState { data, controller })
}

/// A plug-in started off the session's thread.
struct Started {
    key: u64,
    /// Which start of the device this answers; an older one is dropped.
    generation: u64,
    result: Result<(Plugin, PluginAudio, Option<String>), String>,
    /// Its state read just after it started, for later reads to be compared with (a plug-in may
    /// report a state it was given in a form of its own); `None` when it did not answer.
    first_state: Option<SavedState>,
}

/// What a plug-in's control half did on a thread of its own, coming back.
enum Done {
    Opened(Result<(u32, u32), String>),
    Closed(Result<(), String>),
    /// Its state was read (now and then, for a restart, and to notice changes made in its
    /// editor); `None` when it did not answer.
    Saved(Option<SavedState>),
    /// It was stopped; its last state, for when it starts again.
    Stopped(Option<SavedState>),
}

/// A control half back from a thread, or (stopped) only what it left.
struct Returned {
    key: u64,
    generation: u64,
    control: Option<Plugin>,
    done: Done,
}

struct Instance {
    status: PluginStatus,
    plugin: Option<(String, String, &'static str)>,
    info: PluginSlotInfo,
    /// `None` while it starts, or while a thread of its own talks to it.
    control: Option<Plugin>,
    generation: u64,
    state_refused: Option<String>,
    /// Whether its editor is open, as last known.
    editor_open: bool,
    /// How the editor was opened: embedded in the session's window, or in a window of its
    /// own. Set when it is asked to open, so a placement that comes while it opens is kept.
    editor_embedded: bool,
    /// The size of the open editor's inside in pixels, as last known.
    editor_size: Option<(u32, u32)>,
    /// Where the window wants the embedded editor, and what the host was last told.
    placement_wanted: Option<Placement>,
    placement_sent: Option<Placement>,
    /// The editor is being closed to open again the other way (the embedding setting changed).
    reopen_editor: bool,
    /// An embedded editor cannot open on this system (macOS: a view of the host's process
    /// cannot attach to another process's window), so editors open in a window of their own
    /// instead, whatever the setting says.
    force_floating: bool,
    /// How it keeps up with the engine, while its audio half runs there.
    counts: Option<Arc<BlockCounts>>,
    /// Its parameters' ids and ranges (lowest, highest), once listed ([`Rack::params`]).
    ranges: Option<Vec<(u32, f64, f64)>>,
}

impl Instance {
    fn new(status: PluginStatus, plugin: Option<(String, String, &'static str)>, generation: u64) -> Self {
        Self {
            status,
            plugin,
            info: PluginSlotInfo::default(),
            ranges: None,
            control: None,
            generation,
            state_refused: None,
            editor_open: false,
            editor_embedded: false,
            editor_size: None,
            placement_wanted: None,
            placement_sent: None,
            reopen_editor: false,
            force_floating: false,
            counts: None,
        }
    }

    /// The editor is closed (or gone with its process).
    fn editor_gone(&mut self) {
        self.editor_open = false;
        self.editor_size = None;
        self.placement_wanted = None;
        self.placement_sent = None;
    }
}

/// The plug-ins one project runs, while it plays.
pub struct Rack {
    env: Option<std::sync::Arc<PluginEnv>>,
    sample_rate: u32,
    instances: HashMap<u64, Instance>,
    /// The last state of plug-ins that were stopped (a device removed, the project no longer
    /// playing), so they come back as they were.
    kept: HashMap<u64, SavedState>,
    sender: Sender<Started>,
    receiver: Receiver<Started>,
    next_generation: u64,
    checked: Instant,
    saved: Instant,
    /// The window editors open over (owned by it), when the session runs in one.
    owner: Option<u64>,
    /// Editors open embedded in that window's device panel rather than in windows of their own.
    embed: bool,
    returned_sender: Sender<Returned>,
    returned: Receiver<Returned>,
    /// What happened while the project was saved, for the next [`Rack::sync`] to tell.
    notices: Vec<String>,
    /// The states read compared with the ones last saved: changes made in plug-in editors.
    pub(crate) edits: PluginEdits,
}

impl Default for Rack {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        let (returned_sender, returned) = mpsc::channel();
        Self {
            returned_sender,
            returned,
            notices: Vec::new(),
            env: None,
            sample_rate: 0,
            instances: HashMap::new(),
            kept: HashMap::new(),
            sender,
            receiver,
            next_generation: 0,
            checked: Instant::now(),
            saved: Instant::now(),
            owner: None,
            embed: false,
            edits: PluginEdits::default(),
        }
    }
}

impl Rack {
    /// What plug-ins are started with. A new one (after a scan, or another hosting choice)
    /// makes the plug-ins that were not found, or could not start, be looked for again.
    pub fn set_env(&mut self, env: std::sync::Arc<PluginEnv>) {
        if self.env.as_ref().is_some_and(|old| std::sync::Arc::ptr_eq(old, &env)) {
            return;
        }
        self.instances.retain(|_, i| !matches!(i.status, PluginStatus::Missing | PluginStatus::Failed(_)));
        self.env = Some(env);
    }

    pub fn env(&self) -> Option<&PluginEnv> {
        self.env.as_deref()
    }

    /// The state of every plug-in device as it is now, by key, for saving the project. Each
    /// running plug-in is asked on a thread of its own and the answers are waited for at most
    /// [`SAVE_WAIT`], so a host process busy with something else (an editor opening over the
    /// window) cannot hold the window: one that has not answered by then gives the state it last
    /// gave, and a stopped one the state it left.
    pub(crate) fn current_states(&mut self) -> HashMap<u64, SavedState> {
        self.read_states();
        let mut states = self.kept.clone();
        for (key, instance) in &self.instances {
            if let Some(state) = instance.control.as_ref().and_then(Plugin::last_state) {
                states.insert(*key, state.clone());
            }
        }
        states
    }

    /// Asks every running plug-in for its state, each on a thread of its own, and waits for the
    /// answers: at most [`SAVE_WAIT`] in all, with the time it takes a plug-in lent out for
    /// something else (a read just after it started) to come back first.
    fn read_states(&mut self) {
        let deadline = Instant::now() + SAVE_WAIT;
        let mut notices = Vec::new();
        let lent = |rack: &Self, keys: Option<&[u64]>| {
            rack.instances.iter().any(|(key, i)| {
                i.control.is_none() && keys.map_or(i.status == PluginStatus::Running, |keys| keys.contains(key))
            })
        };
        while lent(self, None) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
            self.take_returned(&mut notices);
        }
        let running: Vec<u64> = self.instances.iter().filter(|(_, i)| i.control.is_some()).map(|(k, _)| *k).collect();
        let asked: Vec<u64> = running.into_iter().filter(|key| self.lend(*key, read_state).is_ok()).collect();
        while lent(self, Some(&asked)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
            self.take_returned(&mut notices);
        }
        self.notices.extend(notices);
    }

    /// Reads the running plug-ins' states twice in a row, so a change made in an editor since
    /// the last read counts now, not after the next two reads ([`SAVE_EVERY`] apart): before
    /// the window asks whether to save. Waits at most [`SAVE_WAIT`] for each read.
    pub(crate) fn check_edits(&mut self) {
        self.read_states();
        self.read_states();
    }

    /// Whether a plug-in's state changed since the project was last saved or opened, without
    /// a change to the project itself (a setting made in the plug-in's editor).
    pub(crate) fn edited(&self) -> bool {
        self.edits.changed()
    }

    /// The project was saved with `states` (from [`Rack::current_states`]).
    pub(crate) fn saved(&mut self, states: &HashMap<u64, SavedState>) {
        self.edits.saved(states);
    }

    /// Stops every plug-in, keeping each one's last state for when it starts again. The host
    /// processes are told on threads of their own, so this never waits for them.
    pub fn stop_all(&mut self) {
        let instances: Vec<(u64, Instance)> = self.instances.drain().collect();
        for (key, instance) in instances {
            if let Some(control) = instance.control {
                self.stop(key, control);
            }
        }
    }

    /// Reads `control`'s last state and lets it go, on a thread of its own; the state comes
    /// back to be kept for when the device's plug-in starts again.
    fn stop(&self, key: u64, mut control: Plugin) {
        let sender = self.returned_sender.clone();
        let _detached = std::thread::Builder::new().name("suhina-plugin-stop".into()).spawn(move || {
            let state = (control.health() == Health::Running).then(|| control.save_state().ok()).flatten();
            drop(control);
            let _gone = sender.send(Returned { key, generation: 0, control: None, done: Done::Stopped(state) });
        });
    }

    /// Lends the control half of plug-in device `key` to a thread of its own for `work`, so
    /// the session's thread never waits for a host process (one busy loading a sibling, or
    /// making an editor's window that waits for the session's own window). It comes back
    /// through [`Rack::take_returned`].
    fn lend(&mut self, key: u64, work: impl FnOnce(&mut Plugin) -> Done + Send + 'static) -> Result<(), CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device's plug-in is not running")?;
        let mut control = instance.control.take().ok_or("the plug-in is busy; try again in a moment")?;
        let (sender, generation) = (self.returned_sender.clone(), instance.generation);
        let spawned = std::thread::Builder::new().name("suhina-plugin-control".into()).spawn(move || {
            let done = work(&mut control);
            let _gone = sender.send(Returned { key, generation, control: Some(control), done });
        });
        spawned.map(|_| ()).map_err(|e| CommandError(format!("no thread to talk to the plug-in on: {e}")))
    }

    /// Takes back control halves lent out, and the states of stopped plug-ins.
    fn take_returned(&mut self, notices: &mut Vec<String>) {
        while let Ok(returned) = self.returned.try_recv() {
            let Returned { key, generation, control, done } = returned;
            if let Done::Saved(Some(state)) = &done {
                self.edits.read(key, state);
            }
            if let Done::Stopped(state) = done {
                if let Some(state) = state {
                    self.edits.read(key, &state);
                    self.kept.insert(key, state);
                }
                continue;
            }
            let Some(control) = control else { continue };
            let mut reopen = false;
            let mut fall_back = false;
            match self.instances.get_mut(&key).filter(|i| i.generation == generation && i.control.is_none()) {
                Some(instance) => {
                    let name = instance.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
                    match done {
                        Done::Opened(Ok(size)) => {
                            instance.editor_open = true;
                            instance.editor_size = Some(size);
                        }
                        Done::Opened(Err(why)) => {
                            // Some systems cannot show an editor inside this window's process
                            // (macOS): a window of its own still can. One fall back to it, so
                            // the default setting works everywhere.
                            if instance.editor_embedded && !instance.force_floating {
                                instance.force_floating = true;
                                instance.editor_gone();
                                fall_back = true;
                            } else {
                                notices.push(format!("{name}: its editor did not open ({why})"));
                            }
                        }
                        Done::Closed(result) => {
                            instance.editor_gone();
                            reopen = std::mem::take(&mut instance.reopen_editor);
                            if let Err(why) = result {
                                notices.push(format!("{name}: its editor did not close ({why})"));
                            }
                        }
                        Done::Saved(_) | Done::Stopped(_) => {}
                    }
                    instance.control = Some(control);
                }
                // The device went while its plug-in was out: stop it like the others.
                None => self.stop(key, control),
            }
            if (reopen || fall_back)
                && let Err(e) = self.open_editor(key)
            {
                notices.push(format!("the editor did not open again: {}", e.0));
            }
        }
    }

    /// Tells the hosts where the window wants their embedded editors, where that has not been
    /// told yet (the control half was out when the window said).
    fn send_placements(&mut self) {
        for instance in self.instances.values_mut() {
            let (Some(wanted), Some(control)) = (instance.placement_wanted, instance.control.as_mut()) else {
                continue;
            };
            if instance.placement_sent == Some(wanted) {
                continue;
            }
            if control.place_editor(wanted).unwrap_or(false) {
                instance.placement_sent = Some(wanted);
            }
        }
    }

    /// What is known about the plug-in device `key`.
    pub fn report(&mut self, key: u64) -> Option<PluginReport> {
        let instance = self.instances.get_mut(&key)?;
        if let Some(control) = instance.control.as_mut() {
            if control.editor_open() {
                instance.editor_open = true;
                instance.editor_size = control.editor_size();
            } else {
                instance.editor_gone();
            }
        }
        Some(PluginReport {
            status: instance.status.clone(),
            plugin: instance.plugin.clone(),
            instrument: instance.info.instrument,
            latency: instance.info.latency,
            editor_open: instance.editor_open,
            editor_embedded: instance.editor_size.filter(|_| instance.editor_embedded && instance.editor_open),
            blocks: instance.counts.as_ref().map_or(0, |c| c.read().0),
            late_blocks: instance.counts.as_ref().map_or(0, |c| c.read().1),
            longest_us: instance.counts.as_ref().map_or(0, |c| c.longest_us()),
            no_time: instance.counts.as_ref().map_or(0, |c| c.no_time()),
            state_refused: instance.state_refused.clone(),
        })
    }

    /// How far the plug-ins have got with starting.
    pub fn progress(&self) -> PluginProgress {
        let mut progress = PluginProgress::default();
        let name = |i: &Instance| i.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
        for instance in self.instances.values() {
            match &instance.status {
                PluginStatus::Missing => progress.missing += 1,
                PluginStatus::Running => progress.ready += 1,
                PluginStatus::Loading => progress.loading.push(name(instance)),
                PluginStatus::Crashed(_) | PluginStatus::Failed(_) => progress.failed.push(name(instance)),
            }
        }
        progress.total = self.instances.len() - progress.missing;
        progress.loading.sort();
        progress.failed.sort();
        progress
    }

    /// The slots the engine's graphs are built with.
    fn slot_info(&self) -> HashMap<u64, PluginSlotInfo> {
        self.instances
            .iter()
            .filter(|(_, i)| i.status != PluginStatus::Missing)
            .map(|(key, i)| (*key, i.info))
            .collect()
    }

    /// Brings the running plug-ins in line with `project`: starts the ones new devices need
    /// (in the background), stops the ones whose devices are gone, takes in the ones that have
    /// started and looks after their health. Returns what the user should hear about.
    pub fn sync(&mut self, project: &Project, player: Option<&mut Player>) -> Vec<String> {
        let mut notices = std::mem::take(&mut self.notices);
        let Some(player) = player else {
            self.stop_all();
            return notices;
        };
        let Some(env) = self.env.clone() else { return notices };
        let sample_rate = player.sample_rate();
        if sample_rate != self.sample_rate {
            self.stop_all();
            self.sample_rate = sample_rate;
        }
        let devices = plugin_devices(project);
        let mut changed = false;
        // Devices that are gone.
        let gone: Vec<u64> = self.instances.keys().copied().filter(|k| !devices.iter().any(|d| d.key == *k)).collect();
        for key in gone {
            if let Some(instance) = self.instances.remove(&key) {
                if let Some(control) = instance.control {
                    self.stop(key, control);
                }
                changed = true;
            }
        }
        // New ones.
        let mut missing = Vec::new();
        for found in &devices {
            if self.instances.contains_key(&found.key) {
                continue;
            }
            let Some(device) = project
                .channels
                .iter()
                .find(|c| c.id == found.channel)
                .and_then(|c| suhina_model::device_at(&c.devices, &found.path))
            else {
                continue;
            };
            changed = true;
            let Some((matched, instrument)) = find(&env.catalog, device) else {
                missing.push(device.name.clone());
                self.instances.insert(found.key, Instance::new(PluginStatus::Missing, None, 0));
                continue;
            };
            let state = self.kept.remove(&found.key).or_else(|| saved_state(device));
            let launch = Launch {
                host: env.host.clone(),
                plugin: matched.plugin.spec(),
                channels: 2,
                max_frames: MAX_BLOCK_FRAMES as u32,
                sample_rate: f64::from(sample_rate),
                timeout: START_TIMEOUT,
                group: (env.group)(matched.plugin),
            };
            self.next_generation += 1;
            let generation = self.next_generation;
            let plugin = Some((matched.plugin.name.clone(), matched.plugin.vendor.clone(), matched.how.name()));
            let mut instance = Instance::new(PluginStatus::Loading, plugin, generation);
            instance.info = PluginSlotInfo { instrument, latency: 0 };
            self.instances.insert(found.key, instance);
            spawn_start(self.sender.clone(), found.key, generation, launch, state);
        }
        if !missing.is_empty() {
            notices.push(missing_notice(&env, &missing));
        }
        changed |= self.receive(player, &mut notices);
        self.take_returned(&mut notices);
        changed |= self.follow_latencies();
        self.send_placements();
        self.watch(&mut notices);
        if changed {
            player.set_plugin_info(self.slot_info());
            if let Err(e) = player.update(project) {
                notices.push(format!("plug-ins: {}", e.0));
            }
        }
        notices
    }

    /// Takes in plug-ins that have started: hands each one's audio half to the engine. Returns
    /// whether what the graphs need to know changed (a latency).
    fn receive(&mut self, player: &mut Player, notices: &mut Vec<String>) -> bool {
        let mut changed = false;
        while let Ok(started) = self.receiver.try_recv() {
            let Some(instance) = self.instances.get_mut(&started.key) else { continue };
            if instance.generation != started.generation {
                continue;
            }
            if let Some(state) = &started.first_state {
                self.edits.read(started.key, state);
            }
            let name = instance.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
            match started.result {
                Ok((control, audio, refused)) => {
                    if let Some(why) = &refused {
                        notices.push(format!("{name} did not take its saved settings ({why}); it runs with its own"));
                    }
                    let latency = control.latency();
                    if latency != instance.info.latency {
                        instance.info.latency = latency;
                        changed = true;
                    }
                    instance.state_refused = refused;
                    let hosted = Hosted::new(audio);
                    instance.counts = Some(hosted.counts());
                    match player.send_plugin(started.key, Box::new(hosted)) {
                        Ok(()) => instance.status = PluginStatus::Running,
                        Err(e) => instance.status = PluginStatus::Failed(e.0),
                    }
                    instance.control = Some(control);
                }
                Err(why) => {
                    notices.push(format!("{name} could not start: {why}"));
                    instance.status = PluginStatus::Failed(why);
                }
            }
        }
        changed
    }

    /// Takes in the latencies running plug-ins reported since they started (a lookahead the user
    /// changed, say). Returns whether what the graphs need to know changed: a new graph then
    /// lines the tracks up again.
    fn follow_latencies(&mut self) -> bool {
        let mut changed = false;
        for instance in self.instances.values_mut() {
            if instance.status != PluginStatus::Running {
                continue;
            }
            let Some(control) = instance.control.as_mut() else { continue };
            let latency = control.follow_latency();
            if latency != instance.info.latency {
                instance.info.latency = latency;
                changed = true;
            }
        }
        changed
    }

    /// Notices plug-ins that crashed or hung, and now and then reads each one's state.
    fn watch(&mut self, notices: &mut Vec<String>) {
        if self.checked.elapsed() < HEALTH_EVERY {
            return;
        }
        self.checked = Instant::now();
        let save = self.saved.elapsed() >= SAVE_EVERY;
        if save {
            self.saved = Instant::now();
        }
        let mut to_save = Vec::new();
        for (key, instance) in self.instances.iter_mut() {
            let Some(control) = instance.control.as_mut() else { continue };
            if instance.status != PluginStatus::Running {
                continue;
            }
            let name = instance.plugin.as_ref().map_or("A plug-in", |p| p.0.as_str());
            match control.health() {
                Health::Running => {
                    if save {
                        to_save.push(*key);
                    }
                }
                Health::Crashed => {
                    notices
                        .push(format!("{name} crashed; the project plays on without it (restart it from its device)"));
                    instance.status = PluginStatus::Crashed("its host process exited".into());
                }
                Health::Hung => {
                    notices.push(format!(
                        "{name} stopped answering; the project plays on without it (restart it from its device)"
                    ));
                    instance.status = PluginStatus::Crashed("it stopped answering".into());
                }
            }
        }
        // Kept by the control half for a restart; read where waiting does not matter.
        for key in to_save {
            let _busy_then_next_time = self.lend(key, read_state);
        }
    }

    /// Restarts the plug-in device `key` in a fresh host process with its last state, in the
    /// background; it plays again once it has started.
    pub fn restart(&mut self, key: u64) -> Result<(), CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device has no plug-in running")?;
        if instance.status == PluginStatus::Missing {
            return Err("the plug-in is not installed; there is nothing to restart".into());
        }
        let Some(control) = instance.control.take() else {
            return match instance.status {
                // It never started: start it again from scratch, as when the project opened.
                PluginStatus::Failed(_) | PluginStatus::Crashed(_) => {
                    self.instances.remove(&key);
                    Ok(())
                }
                PluginStatus::Loading => Err("the plug-in is still starting".into()),
                _ => Err("the plug-in is busy; try again in a moment".into()),
            };
        };
        self.next_generation += 1;
        instance.generation = self.next_generation;
        instance.status = PluginStatus::Loading;
        let (sender, generation) = (self.sender.clone(), self.next_generation);
        let spawned = std::thread::Builder::new().name("suhina-plugin-restart".into()).spawn(move || {
            let mut control = control;
            let result = control.restart().map(|audio| (control, audio, None)).map_err(|e| e.to_string());
            let _gone = sender.send(Started { key, generation, result, first_state: None });
        });
        spawned.map(|_| ()).map_err(|e| CommandError(format!("could not restart the plug-in: {e}")))
    }

    /// The window editors open over from now on: a Win32 `HWND` or an X11 window id.
    pub fn set_owner(&mut self, owner: Option<u64>) {
        self.owner = owner;
    }

    /// Whether editors open embedded in the owner window's device panel from now on, rather
    /// than in windows of their own (nothing without an owner window). An editor open the
    /// other way is closed and opened again the new way.
    pub fn set_embedding(&mut self, embed: bool) {
        let embed = embed && self.owner.is_some();
        if self.embed == embed {
            return;
        }
        self.embed = embed;
        let to_move: Vec<u64> = self
            .instances
            .iter()
            .filter(|(_, i)| (i.editor_open && i.editor_embedded != embed) || (!embed && i.force_floating))
            .map(|(key, _)| *key)
            .collect();
        for key in to_move {
            if let Some(instance) = self.instances.get_mut(&key) {
                instance.force_floating = false;
            }
            if self.close_editor(key).is_ok()
                && let Some(instance) = self.instances.get_mut(&key)
            {
                instance.reopen_editor = true;
            }
        }
    }

    /// Opens the editor of the plug-in device `key`; returns the size of its inside. Over an
    /// owner window (the desktop app's) it opens on a thread of its own and `None` comes back
    /// at once: the host making a window that the app's window owns waits for the app's window
    /// thread, which must not be waiting for the host meanwhile. Embedded (the setting), it
    /// opens hidden inside that window until [`Rack::place_editor`] puts it somewhere.
    /// The parameters of the plug-in device `key` as its plug-in lists them now (it waits for
    /// the host's answer), remembered so values set later can be kept in range.
    pub fn params(&mut self, key: u64) -> Result<Vec<suhina_plugins::ParamInfo>, CommandError> {
        let list = self.running(key)?.params().map_err(|e| CommandError(e.to_string()))?;
        if let Some(instance) = self.instances.get_mut(&key) {
            let ranges = list.iter().map(|p| (p.id, p.min.min(p.max), p.max.max(p.min))).collect();
            instance.ranges = Some(ranges);
        }
        Ok(list)
    }

    /// The range of parameter `id` of the plug-in device `key`: `None` when its parameters
    /// have not been listed, `Some(None)` when they have and it has no such parameter.
    pub fn param_range(&self, key: u64, id: u32) -> Option<Option<(f64, f64)>> {
        let ranges = self.instances.get(&key)?.ranges.as_ref()?;
        Some(ranges.iter().find(|(i, _, _)| *i == id).map(|&(_, min, max)| (min, max)))
    }

    pub fn open_editor(&mut self, key: u64) -> Result<Option<(u32, u32)>, CommandError> {
        self.running(key)?;
        match self.owner {
            Some(owner) => {
                let embed = self.embed && !self.instances.get(&key).is_some_and(|i| i.force_floating);
                self.lend(key, move |control| {
                    let opened = if embed {
                        control.open_editor_embedded(owner)
                    } else {
                        control.open_editor_owned(Some(owner))
                    };
                    Done::Opened(opened.map_err(|e| e.to_string()))
                })?;
                if let Some(instance) = self.instances.get_mut(&key) {
                    instance.editor_embedded = embed;
                    instance.placement_sent = None;
                }
                Ok(None)
            }
            None => self.running(key)?.open_editor().map(Some).map_err(|e| CommandError(e.to_string())),
        }
    }

    /// Where the window shows the embedded editors this frame, by plug-in device key; every
    /// other embedded editor is hidden (its device is not in the panel). Told to the hosts now
    /// where it can be (and the place changed), otherwise at the next [`Rack::sync`]. Never
    /// waits for a host. Nothing for editors in windows of their own.
    pub fn place_editors(&mut self, shown: &[(u64, Placement)]) {
        for (key, instance) in &mut self.instances {
            if !instance.editor_embedded || !instance.editor_open {
                continue;
            }
            let placement = shown.iter().find(|(k, _)| k == key).map_or(Placement::HIDDEN, |(_, p)| *p);
            instance.placement_wanted = Some(placement);
            if instance.placement_sent == Some(placement) {
                continue;
            }
            if let Some(control) = instance.control.as_mut()
                && control.place_editor(placement).unwrap_or(false)
            {
                instance.placement_sent = Some(placement);
            }
        }
    }

    /// Closes the editor of the plug-in device `key`; with an owner window, on a thread of
    /// its own, like opening it.
    pub fn close_editor(&mut self, key: u64) -> Result<(), CommandError> {
        self.running(key)?;
        if self.owner.is_some() {
            return self.lend(key, |control| Done::Closed(control.close_editor().map_err(|e| e.to_string())));
        }
        self.running(key)?.close_editor().map_err(|e| CommandError(e.to_string()))
    }

    fn running(&mut self, key: u64) -> Result<&mut Plugin, CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device's plug-in is not running")?;
        match &instance.status {
            PluginStatus::Running => {
                instance.control.as_mut().ok_or_else(|| "the plug-in is busy; try again in a moment".into())
            }
            PluginStatus::Loading => Err("the plug-in is still starting".into()),
            PluginStatus::Missing => Err("the plug-in is not installed".into()),
            PluginStatus::Crashed(why) => Err(format!("the plug-in crashed ({why}); restart it first").into()),
            PluginStatus::Failed(why) => Err(format!("the plug-in could not start: {why}").into()),
        }
    }

    /// Waits until no plug-in is starting any more, taking each in as it comes (tests, and
    /// callers that need every plug-in before going on), at most `timeout`.
    pub fn wait_started(&mut self, project: &Project, player: &mut Player, timeout: Duration) -> Vec<String> {
        let deadline = Instant::now() + timeout;
        let mut notices = self.sync(project, Some(player));
        while self.instances.values().any(|i| i.status == PluginStatus::Loading) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            notices.extend(self.sync(project, Some(player)));
        }
        notices
    }
}

/// What the status bar says about plug-ins that were not found: why, when the catalog is empty
/// (no scan yet, or the host to scan with is missing), else which.
fn missing_notice(env: &PluginEnv, missing: &[String]) -> String {
    if env.catalog.plugins().next().is_none() {
        if !env.host.exists() {
            return format!(
                "{} plug-ins cannot be looked for: the plug-in host {} is missing",
                missing.len(),
                env.host.display()
            );
        }
        return format!(
            "{} plug-ins cannot be found yet: no plug-in scan has run (Settings › Plug-ins › Scan)",
            missing.len()
        );
    }
    format!(
        "{} plug-in{} not installed, kept with {} settings: {}",
        missing.len(),
        if missing.len() == 1 { " is" } else { "s are" },
        if missing.len() == 1 { "its" } else { "their" },
        missing.join(", ")
    )
}

/// Reads a plug-in's state; its control half keeps it for a restart.
fn read_state(control: &mut Plugin) -> Done {
    Done::Saved(control.save_state().ok())
}

/// Starts a plug-in on a thread of its own and sends the result.
fn spawn_start(sender: Sender<Started>, key: u64, generation: u64, launch: Launch, state: Option<SavedState>) {
    let fallback = sender.clone();
    let spawned = std::thread::Builder::new().name("suhina-plugin-start".into()).spawn(move || {
        let mut result = start(launch, state).map_err(|e| e.to_string());
        let first_state = result.as_mut().ok().and_then(|(plugin, _, _)| plugin.save_state().ok());
        let _gone = sender.send(Started { key, generation, result, first_state });
    });
    if let Err(e) = spawned {
        let result = Err(format!("no thread to start it on: {e}"));
        let _gone = fallback.send(Started { key, generation, result, first_state: None });
    }
}

/// Starts the plug-in `launch` names and gives it `state`. A plug-in that refuses the state
/// still runs, with its own defaults; the reason comes back.
fn start(launch: Launch, state: Option<SavedState>) -> Result<(Plugin, PluginAudio, Option<String>), PluginError> {
    let (mut plugin, audio) = Plugin::start(launch)?;
    let mut refused = None;
    if let Some(state) = state {
        match plugin.load_state(state) {
            Ok(()) => {}
            Err(PluginError::Host(why)) => refused = Some(why),
            Err(other) => return Err(other),
        }
    }
    Ok((plugin, audio, refused))
}

/// A project's plug-ins started for an offline render (ADR 0006: offline renders run the same
/// plug-ins through the same engine, without a deadline). Keep it until the render is done:
/// dropping it ends the host processes.
pub struct OfflinePlugins {
    /// For the renderer.
    pub plugins: Plugins,
    controls: Vec<Plugin>,
    /// What the user should hear about: missing plug-ins, ones that could not start or did not
    /// take their state.
    pub notices: Vec<String>,
}

impl OfflinePlugins {
    /// No plug-ins: a render without a catalog passes plug-in devices through.
    pub fn none() -> Self {
        Self { plugins: Plugins::new(), controls: Vec::new(), notices: Vec::new() }
    }

    /// Plug-ins running for this render.
    pub fn running(&self) -> usize {
        self.controls.len()
    }

    /// The ids of the host processes the running plug-ins are in, in the order they started:
    /// consecutive plug-ins in one process share a round trip a block (ADR 0006, decision 2).
    pub fn process_ids(&self) -> Vec<u32> {
        self.controls.iter().map(Plugin::process_id).collect()
    }
}

/// Starts every plug-in device of `project` that is installed, at `sample_rate`, and waits for
/// each.
pub fn start_offline(project: &Project, env: &PluginEnv, sample_rate: u32) -> OfflinePlugins {
    let mut offline = OfflinePlugins { plugins: Plugins::new(), controls: Vec::new(), notices: Vec::new() };
    let mut missing = Vec::new();
    for found in plugin_devices(project) {
        let Some(device) = project
            .channels
            .iter()
            .find(|c| c.id == found.channel)
            .and_then(|c| suhina_model::device_at(&c.devices, &found.path))
        else {
            continue;
        };
        let Some((matched, instrument)) = find(&env.catalog, device) else {
            missing.push(device.name.clone());
            continue;
        };
        let launch = Launch {
            host: env.host.clone(),
            plugin: matched.plugin.spec(),
            channels: 2,
            max_frames: MAX_BLOCK_FRAMES as u32,
            sample_rate: f64::from(sample_rate),
            timeout: START_TIMEOUT,
            group: (env.group)(matched.plugin),
        };
        match start(launch, saved_state(device)) {
            Ok((control, audio, refused)) => {
                if let Some(why) = refused {
                    offline.notices.push(format!("{} did not take its saved settings ({why})", device.name));
                }
                let info = PluginSlotInfo { instrument, latency: control.latency() };
                offline.plugins.insert(found.key, info, Box::new(Hosted::new(audio)));
                offline.controls.push(control);
            }
            Err(e) => offline.notices.push(format!("{} could not start: {e}", device.name)),
        }
    }
    if !missing.is_empty() {
        offline.notices.push(format!("not installed, passed through: {}", missing.join(", ")));
    }
    offline
}

/// The device kind a project stores for the installed plug-in `plugin`.
pub fn device_kind(plugin: &PluginInfo) -> DeviceKind {
    match plugin.format() {
        "vst3" => DeviceKind::Vst3 { id: plugin.id.clone() },
        _ => DeviceKind::Clap { id: plugin.id.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_each_state() {
        let mut rack = Rack::default();
        let instance =
            |status: PluginStatus, name: &str| Instance::new(status, Some((name.to_string(), "V".into(), "same")), 0);
        rack.instances.insert(1, instance(PluginStatus::Running, "A"));
        rack.instances.insert(2, instance(PluginStatus::Loading, "C"));
        rack.instances.insert(3, instance(PluginStatus::Loading, "B"));
        rack.instances.insert(4, instance(PluginStatus::Missing, "D"));
        rack.instances.insert(5, instance(PluginStatus::Failed("no".into()), "E"));
        assert_eq!(
            rack.progress(),
            PluginProgress {
                total: 4,
                ready: 1,
                loading: vec!["B".into(), "C".into()],
                missing: 1,
                failed: vec!["E".into()]
            }
        );
        // A new catalog looks for the missing and failed ones again.
        rack.set_env(std::sync::Arc::new(PluginEnv::new(Catalog::new(), PathBuf::from("host"))));
        rack.set_env(std::sync::Arc::new(PluginEnv::new(Catalog::new(), PathBuf::from("host"))));
        assert_eq!(rack.progress().missing, 0);
        assert!(rack.progress().failed.is_empty());
        assert_eq!(rack.progress().total, 3);
    }

    #[test]
    fn releases_of_a_block_not_sent_are_kept_for_the_next() {
        let events = [
            Event::note_on(1, 0, 60, 1.0),
            Event::note_off(2, 0, 61, 0.5),
            Event { frame: 3, note: -1, kind: EventKind::Controller { channel: 0, number: SUSTAIN, value: 0.0 } },
            Event { frame: 3, note: -1, kind: EventKind::Controller { channel: 0, number: 1, value: 0.3 } },
        ];
        let mut carried = Vec::with_capacity(2);
        carry_releases(&events, &mut carried);
        assert_eq!(carried, [events[1], events[2]]);
        // Never past the room made for them.
        carry_releases(&events, &mut carried);
        assert_eq!(carried.len(), 2);
    }

    #[test]
    fn parameter_values_of_a_block_not_sent_are_kept_the_latest_of_each() {
        let events =
            [Event::param(0, 3, 0.1), Event::note_on(1, 0, 60, 1.0), Event::param(5, 3, 0.2), Event::param(6, 4, 1.0)];
        let mut carried = Vec::with_capacity(4);
        carry_releases(&events, &mut carried);
        assert_eq!(carried, [Event::param(5, 3, 0.2), Event::param(6, 4, 1.0)]);
    }

    #[test]
    fn parameter_values_and_notes_merge_in_frame_order() {
        let params = [Event::param(0, 1, 0.5), Event::param(4, 1, 0.75)];
        let notes = [Event::note_on(0, 0, 60, 1.0), Event::note_off(2, 0, 60, 0.5), Event::note_on(4, 0, 62, 1.0)];
        let mut out = Vec::with_capacity(8);
        merge(&params, &notes, &mut out);
        assert_eq!(out, [params[0], notes[0], notes[1], params[1], notes[2]]);
        // Never past the room made for them.
        let mut short = Vec::with_capacity(2);
        merge(&params, &notes, &mut short);
        assert_eq!(short, [params[0], notes[0]]);
    }

    #[test]
    fn notes_become_the_hosts_events() {
        let mut out = Vec::with_capacity(8);
        push_event(&mut out, 3, DeviceEvent::On { key: 60, velocity: 2.0 });
        push_event(&mut out, 4, DeviceEvent::Off { key: 200, velocity: 0.5 });
        push_event(&mut out, 5, DeviceEvent::PitchBend(-3.0));
        push_event(&mut out, 6, DeviceEvent::Pressure(0.5));
        push_event(&mut out, 7, DeviceEvent::AllNotesOff);
        assert_eq!(
            out,
            [
                Event { frame: 3, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } },
                Event { frame: 5, note: -1, kind: EventKind::PitchBend { channel: 0, value: -1.0 } },
                Event { frame: 7, note: -1, kind: EventKind::Controller { channel: 0, number: 64, value: 0.0 } },
                Event { frame: 7, note: -1, kind: EventKind::Controller { channel: 0, number: 123, value: 0.0 } },
            ]
        );
        // Never more than the room made for them.
        let mut full = Vec::with_capacity(1);
        push_event(&mut full, 0, DeviceEvent::AllNotesOff);
        assert_eq!(full.len(), 1);
    }

    #[test]
    fn instruments_are_told_by_their_features() {
        let plugin = |features: &[&str]| PluginInfo {
            path: PathBuf::from("x.vst3"),
            id: "x".into(),
            name: "X".into(),
            vendor: "V".into(),
            version: "1".into(),
            features: features.iter().map(|f| f.to_string()).collect(),
            replaces: Vec::new(),
        };
        assert!(is_instrument(&plugin(&["Instrument|Synth"])));
        assert!(is_instrument(&plugin(&["instrument", "stereo"])));
        assert!(!is_instrument(&plugin(&["Fx|Reverb"])));
        assert!(!is_instrument(&plugin(&[])));
        assert_eq!(device_kind(&plugin(&[])), DeviceKind::Vst3 { id: "x".into() });
    }
}
