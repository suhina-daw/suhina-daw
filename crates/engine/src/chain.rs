// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device chains on tracks (ADR 0004 › 9).
//!
//! A track whose channel holds built-in effects and instruments gets a [`Chain`]: everything the track plays in
//! a block (its sine voices, audio clips, launched clips) is summed into the chain's buffer
//! instead of the output, the devices run over it in order, and only then do the track's pan
//! and volume apply. When the first device is an instrument, the track's notes go to it as
//! [`NoteEvent`]s instead of starting sine voices; the chain runs in pieces split at each event,
//! so a note lands on its frame. A track without devices is mixed straight into the output, as
//! before.
//!
//! A plug-in device takes a [`PluginSlot`] in the chain (ADR 0006): it gets the block's events
//! with their frames and runs once per block, so the devices run one after another over the
//! whole block, each split at the events, rather than piece by piece. The output is the same
//! for built-in effects and instruments, which only ever see their own input.
//!
//! Consecutive plug-ins that run in the same host process run as one: the first takes the
//! block with the others named after it, and the host runs them all in one round trip
//! ([`PluginProcessor::send_joint`], ADR 0006 decision 2). A disabled plug-in between them is
//! passed over; a built-in device, or a plug-in in another process or not running, ends the run.
//!
//! A group track whose channel holds devices is a bus (see `buses`): the chains of the tracks
//! that play into it add their output to its buffer instead of the output, and its devices run
//! over the sum.

use std::time::{Duration, Instant};

use suhina_device_api::{Device, NoteEvent, ProcessContext, Role};
use suhina_model::{DeviceKind, Project, TrackId};

use crate::automation::{DeviceLanes, MixLanes};
use crate::plugin::{
    Delay, Followed, MAX_JOINED, OFFLINE_TIMEOUT, PluginProcessor, PluginSlot, Plugins, Sent, plugin_devices,
};
use crate::plugin_params::SlotParams;
use crate::sends::SendOut;

/// The most frames one block of a chain holds; the renderer splits longer blocks.
pub(crate) const MAX_BLOCK: usize = 1024;
/// Events a chain keeps for one block before it runs up to the frame it has reached.
const MAX_PENDING: usize = 256;
const NOT_HELD: u64 = u64::MAX;
/// Gain of a centred track at unity volume, which the sources of a chain track are mixed at
/// (see `track_gains`); the track's real gains apply after the chain, divided by this.
pub(crate) const CHAIN_INPUT_GAIN: f32 = 0.25;

/// One device of a chain: built in, or a plug-in in a host process.
pub(crate) enum Slot {
    Device(Box<dyn Device>),
    Sd(suhina_device_sdk::PreparedEffect),
    InstrumentSd(suhina_device_sdk::PreparedEffect),
    MidiSd(suhina_device_sdk::PreparedMidiEffect),
    Plugin(PluginSlot),
}

impl Slot {
    /// Sets parameter `index` of a data-only (`.sd`) device; other devices are left alone.
    fn set_sd_parameter(&mut self, index: usize, value: f64) {
        match self {
            Slot::Sd(effect) | Slot::InstrumentSd(effect) => effect.set_parameter(index, value),
            Slot::MidiSd(effect) => effect.set_parameter(index, value),
            Slot::Device(_) | Slot::Plugin(_) => {}
        }
    }
}

pub(crate) struct Chain {
    pub(crate) track: TrackId,
    /// The launcher column of the track, or `NO_COLUMN`.
    pub(crate) column: u32,
    slots: Vec<Slot>,
    /// Each device's index among the channel's devices in the project (disabled built-in,
    /// unknown and missing devices have no place in the chain).
    positions: Vec<usize>,
    /// The project's device ids in order (a plug-in's by its key), so a graph swap can tell
    /// whether a new chain holds the same devices as an old one.
    ids: Vec<String>,
    sample_slices: Vec<Option<std::sync::Arc<[f64]>>>,
    /// Every parameter's value from the project (its default when the project has none), per
    /// device.
    params: Vec<Vec<f64>>,
    pub(crate) instrument: bool,
    /// The delay its devices add together, in frames.
    pub(crate) latency: u32,
    /// Latency compensation: delays the chain's output so that it lines up with the chain with
    /// the most latency (set by the graph once every chain is built).
    pub(crate) delay: Delay,
    /// The track's gains after the chain.
    post: (f32, f32),
    /// The lanes of its built-in devices' parameters.
    device_lanes: DeviceLanes,
    /// The lanes of the track's mixer, which scale `post` frame by frame (#395).
    pub(crate) mix: Option<MixLanes>,
    /// The index of the bus chain this chain's output goes into, or `None` for the output. Bus
    /// chains come after the chains that feed them, so they run after them in a block.
    pub(crate) target: Option<usize>,
    /// Some chain plays into this one (a group bus): it runs after them, in order, rather than
    /// in the stages the other chains run their plug-ins in together. Set with `target`.
    pub(crate) is_bus: bool,
    /// The track's cue is on: its output after the chain is also added to the cue bus, at
    /// these gains (its own channel's volume and pan, divided by `CHAIN_INPUT_GAIN`).
    pub(crate) cue: Option<(f32, f32)>,
    /// The returns this chain sends to (see `sends`), each after its devices.
    pub(crate) sends: Vec<SendOut>,
    /// What a send reads through its delay line; empty without sends.
    pub(crate) scratch: Vec<f32>,
    pub(crate) buffer: Vec<f32>,
    pending: Vec<(usize, NoteEvent)>,
    /// The pending events with their frames counted from the start of the span a plug-in runs.
    span_events: Vec<(usize, NoteEvent)>,
    midi_scratch: Vec<(usize, NoteEvent)>,
    /// The current block's: when plug-ins must have answered (`None` offline), and the
    /// transport.
    deadline: Option<Instant>,
    context: ProcessContext,
    /// Frames of the current block the devices have run over.
    done: usize,
    /// When the block runs in stages with other chains ([`Chain::advance`]): the next slot to
    /// run, and the plug-in slot whose block is out, if one is.
    stage: usize,
    waiting: Option<usize>,
    /// When the waiting slot's block went out, for an offline render's patience.
    sent_at: Option<Instant>,
    /// The plug-in slot that is waited for to finish a block it was late with before this
    /// block goes to it (#506); `waiting` names it too while it is.
    behind: Option<usize>,
    /// The plug-in slot that has been waited for that way in this block: it gets the block now,
    /// or has it refused once the deadline is over, without being waited for again.
    caught_up: Option<usize>,
    /// The plug-ins that follow the waiting one in its joint block.
    joined: Joined,
    /// Per key, the renderer frame its note ends at, or `NOT_HELD`.
    held: [u64; 128],
    next_off: u64,
}

impl Chain {
    pub(crate) fn sample_positions(&self) -> Option<(usize, [Option<f32>; 32])> {
        self.slots.iter().zip(&self.positions).find_map(|(slot, &position)| match slot {
            Slot::Device(device) => {
                let positions = device.sample_positions();
                positions.iter().any(Option::is_some).then_some((position, positions))
            }
            _ => None,
        })
    }

    /// The chain for `track` at `sample_rate`, or `None` when its channel holds no device
    /// Suhina can run (other DAWs' devices and plug-ins that are not installed pass the sound
    /// through). Plug-in devices take their processors from `plugins` (one without a processor
    /// yet gets a slot that waits for it). `post` is the track's gains. Allocates: call it off
    /// the audio thread.
    pub(crate) fn build(
        project: &Project,
        audio: &crate::AudioPool,
        track: TrackId,
        column: u32,
        post: (f32, f32),
        sample_rate: u32,
        plugins: &mut Plugins,
    ) -> Option<Self> {
        let channel = project.tracks.iter().find(|t| t.id == track)?.channel?;
        let channel = project.channels.iter().find(|c| c.id == channel)?;
        let keys = plugin_devices(project);
        let frames_per_beat = 60.0 / project.transport.tempo_bpm * f64::from(sample_rate);
        let mut slots: Vec<Slot> = Vec::new();
        let mut positions = Vec::new();
        let mut ids = Vec::new();
        let mut params = Vec::new();
        let mut latency = 0u32;
        let mut device_lanes = DeviceLanes::none();
        for (position, stored) in channel.devices.iter().enumerate() {
            let id = match &stored.kind {
                DeviceKind::Builtin(id) if stored.enabled => id,
                DeviceKind::Sd => {
                    let Some(bytes) = stored.state.as_deref() else { continue };
                    let Ok(text) = std::str::from_utf8(bytes) else { continue };
                    let Ok(graph) = suhina_device_sdk::DeviceGraph::from_sd(text) else { continue };
                    if !stored.enabled {
                        continue;
                    }
                    let mut slot = match graph.role {
                        suhina_device_sdk::DeviceRole::Effect => {
                            let Ok(effect) = graph.compile_at(f64::from(sample_rate)) else { continue };
                            Slot::Sd(effect)
                        }
                        suhina_device_sdk::DeviceRole::MidiEffect => {
                            let Ok(effect) = graph.compile_midi() else { continue };
                            Slot::MidiSd(effect)
                        }
                        suhina_device_sdk::DeviceRole::Instrument => {
                            let Ok(instrument) = graph.compile_instrument_at(f64::from(sample_rate)) else {
                                continue;
                            };
                            Slot::InstrumentSd(instrument)
                        }
                    };
                    let slot_index = slots.len();
                    device_lanes.add_sd(slot_index, &graph.parameters, &stored.automation, frames_per_beat);
                    let mut values = Vec::with_capacity(graph.parameters.len());
                    for (index, parameter) in graph.parameters.iter().enumerate() {
                        let stored_value = stored
                            .params
                            .iter()
                            .find(|value| value.id == parameter.id)
                            .map_or(parameter.default, |value| parameter.clamp(value.value));
                        let value = device_lanes.initial(slot_index, index).unwrap_or(stored_value);
                        slot.set_sd_parameter(index, value);
                        values.push(stored_value);
                    }
                    params.push(values);
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    use std::hash::{Hash, Hasher};
                    bytes.hash(&mut hasher);
                    slots.push(slot);
                    positions.push(position);
                    ids.push(format!("sd:{}:{:016x}", graph.id, hasher.finish()));
                    continue;
                }
                DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. } => {
                    let Some(key) =
                        keys.iter().find(|d| d.channel == channel.id && d.position == position).map(|d| d.key)
                    else {
                        continue;
                    };
                    let Some(&info) = plugins.info.get(&key) else { continue };
                    if stored.enabled {
                        latency = latency.saturating_add(info.latency);
                    }
                    slots.push(Slot::Plugin(PluginSlot {
                        key,
                        processor: plugins.processors.remove(&key),
                        instrument: info.instrument,
                        latency: info.latency,
                        enabled: stored.enabled,
                        missed: 0,
                        params: SlotParams::new(stored, frames_per_beat),
                    }));
                    positions.push(position);
                    ids.push(format!("plugin:{key:016x}"));
                    params.push(Vec::new());
                    continue;
                }
                _ => continue,
            };
            let Some(mut device) = suhina_device_registry::create(id) else { continue };
            if let Some(source) = stored.sample_file().and_then(|file| audio.files.get(file)) {
                device.load_shared_sample(source.sample_rate, source.channels, source.clone());
            }
            device.load_sample_slices(stored.sample_slices().map(std::sync::Arc::from));
            if device.prepare(f64::from(sample_rate)).is_err() {
                continue;
            }
            let info = device.info();
            let mut values: Vec<f64> = info.params.iter().map(|p| p.default).collect();
            if let Some(graph) = stored
                .sd_definition()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
            {
                for p in &graph.parameters {
                    if let Some(value) = info.param_index(&p.id).and_then(|i| values.get_mut(i)) {
                        *value = p.default;
                    }
                }
            }
            for p in &stored.params {
                if let Some(slot) = info.param_index(&p.id).and_then(|i| values.get_mut(i)) {
                    *slot = p.value;
                }
            }
            device_lanes.add(slots.len(), device.as_ref(), &stored.automation, frames_per_beat);
            for (index, &value) in values.iter().enumerate() {
                // A parameter that follows a lane starts where the lane starts.
                device.set_param(index, device_lanes.initial(slots.len(), index).unwrap_or(value));
            }
            device.reset();
            latency = latency.saturating_add(device.latency());
            slots.push(Slot::Device(device));
            positions.push(position);
            ids.push(stored.sample_file().map_or_else(
                || id.clone(),
                |file| {
                    let source = audio.files.get(file).map(std::sync::Arc::as_ptr);
                    format!("{id}:{file}:{source:?}")
                },
            ));
            params.push(values);
        }
        let instrument = slots.iter().any(|slot| match slot {
            Slot::Device(device) => device.info().role == Role::Instrument,
            Slot::Plugin(slot) => slot.instrument,
            Slot::InstrumentSd(_) => true,
            Slot::Sd(_) | Slot::MidiSd(_) => false,
        });
        (!slots.is_empty()).then(|| Self {
            track,
            column,
            slots,
            sample_slices: positions
                .iter()
                .map(|&i| {
                    channel.devices.get(i).and_then(suhina_model::Device::sample_slices).map(std::sync::Arc::from)
                })
                .collect(),
            positions,
            ids,
            params,
            instrument,
            latency,
            delay: Delay::new(0),
            post: (post.0 / CHAIN_INPUT_GAIN, post.1 / CHAIN_INPUT_GAIN),
            device_lanes,
            mix: None,
            target: None,
            is_bus: false,
            cue: None,
            sends: Vec::new(),
            scratch: Vec::new(),
            buffer: vec![0.0; MAX_BLOCK * 2],
            pending: Vec::with_capacity(MAX_PENDING * 2),
            span_events: Vec::with_capacity(MAX_PENDING * 2),
            midi_scratch: Vec::with_capacity(MAX_PENDING * 2),
            deadline: None,
            context: ProcessContext::default(),
            done: 0,
            stage: 0,
            waiting: None,
            sent_at: None,
            behind: None,
            caught_up: None,
            joined: Joined::default(),
            held: [NOT_HELD; 128],
            next_off: NOT_HELD,
        })
    }

    /// A chain without devices for a track that plays into a bus, so its sound reaches the bus
    /// at its gains and with its share of latency compensation. Allocates.
    pub(crate) fn passthrough(track: TrackId, column: u32) -> Self {
        Self {
            track,
            column,
            slots: Vec::new(),
            positions: Vec::new(),
            ids: Vec::new(),
            sample_slices: Vec::new(),
            params: Vec::new(),
            instrument: false,
            latency: 0,
            delay: Delay::new(0),
            post: (1.0, 1.0),
            device_lanes: DeviceLanes::none(),
            mix: None,
            target: None,
            is_bus: false,
            cue: None,
            sends: Vec::new(),
            scratch: Vec::new(),
            buffer: vec![0.0; MAX_BLOCK * 2],
            pending: Vec::new(),
            span_events: Vec::new(),
            midi_scratch: Vec::new(),
            deadline: None,
            context: ProcessContext::default(),
            done: 0,
            stage: 0,
            waiting: None,
            sent_at: None,
            behind: None,
            caught_up: None,
            joined: Joined::default(),
            held: [NOT_HELD; 128],
            next_off: NOT_HELD,
        }
    }

    /// Sets the gains applied after the devices: the track's gains up to where its sound goes.
    pub(crate) fn set_post(&mut self, post: (f32, f32)) {
        self.post = (post.0 / CHAIN_INPUT_GAIN, post.1 / CHAIN_INPUT_GAIN);
    }

    /// Sets the gains the chain's output is added to the cue bus at, or `None` for no cue.
    pub(crate) fn set_cue(&mut self, gains: Option<(f32, f32)>) {
        self.cue = gains.map(|(left, right)| (left / CHAIN_INPUT_GAIN, right / CHAIN_INPUT_GAIN));
    }

    /// Whether it runs devices (a passthrough chain has none).
    pub(crate) fn has_devices(&self) -> bool {
        !self.slots.is_empty()
    }

    /// The first `frames` frames of the buffer, for the chains that play into this bus to add
    /// their output to.
    pub(crate) fn input(&mut self, frames: usize) -> &mut [f32] {
        let end = (frames.min(MAX_BLOCK) * 2).min(self.buffer.len());
        self.buffer.get_mut(..end).unwrap_or_default()
    }

    /// Clears the buffer for a block of `frames` frames, whose plug-ins must answer by
    /// `deadline` (`None` offline). Events queued since the last block stay; they are at frame
    /// 0.
    pub(crate) fn begin(&mut self, frames: usize, deadline: Option<Instant>) {
        if let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) {
            buffer.fill(0.0);
        }
        self.done = 0;
        self.stage = 0;
        self.waiting = None;
        self.sent_at = None;
        self.behind = None;
        self.caught_up = None;
        self.deadline = deadline;
    }

    /// Plans the parameter events of its plug-ins for a block of `frames` frames starting at
    /// arrangement frame `time` (`None` while the transport is stopped): call it after
    /// [`Chain::begin`], before any of the block runs. Never allocates.
    pub(crate) fn automate(&mut self, time: Option<u64>, frames: usize) {
        let frames = frames.min(MAX_BLOCK);
        for slot in self.plugin_slots() {
            slot.params.plan(time, frames);
        }
        self.device_lanes.plan(time, frames);
    }

    /// Fills the track's mixer gains for a block of `frames` frames whose first frame is at
    /// arrangement frame `time` (the play start marker while stopped, with `running` false).
    /// Never allocates.
    pub(crate) fn automate_mix(&mut self, time: u64, frames: usize, running: bool) {
        if let Some(mix) = self.mix.as_mut() {
            mix.plan(time, frames.min(MAX_BLOCK), running);
        }
    }

    /// Runs the devices over the rest of the block as far as it can go without waiting, for
    /// chains that run their plug-ins at the same time: built-in effects and instruments and
    /// `.sd` devices run here, with the block's `context`; at the next plug-in that takes the
    /// block without waiting, the block is handed over and this returns true, for
    /// [`Chain::answered`] to tell when its answer is in and [`Chain::collect`] to take it, after
    /// which this goes on from there. Returns false once every device has run (a plug-in that
    /// cannot take a block that way runs as in [`Chain::complete`]).
    pub(crate) fn advance(&mut self, frames: usize, context: &ProcessContext) -> bool {
        self.context = *context;
        if self.waiting.is_some() {
            return true;
        }
        let to = frames.min(MAX_BLOCK);
        let Self {
            slots,
            buffer,
            pending,
            span_events,
            midi_scratch,
            done,
            deadline,
            context,
            stage,
            waiting,
            device_lanes,
            joined,
            sent_at,
            behind,
            caught_up,
            ..
        } = self;
        let from = (*done).min(to);
        while let Some(slot) = slots.get_mut(*stage) {
            match slot {
                Slot::Device(device) => {
                    run_device(device, *stage, buffer, from, to, pending, device_lanes, context);
                    *stage += 1;
                }
                Slot::Sd(effect) => {
                    run_sd_effect(effect, *stage, buffer, device_lanes, SdRun { from, to, context });
                    *stage += 1;
                }
                Slot::InstrumentSd(instrument) => {
                    span_events.clear();
                    let last = (to - from).saturating_sub(1);
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    run_sd_instrument(
                        instrument,
                        *stage,
                        buffer,
                        span_events,
                        midi_scratch,
                        device_lanes,
                        SdRun { from, to, context },
                    );
                    *stage += 1;
                }
                Slot::MidiSd(effect) => {
                    run_sd_midi(effect, *stage, pending, midi_scratch, device_lanes, SdRun { from, to, context });
                    std::mem::swap(pending, midi_scratch);
                    *stage += 1;
                }
                Slot::Plugin(plugin) => {
                    // A plug-in still at a block it was late with is waited for like an answer,
                    // until this block's deadline, while the other chains go on (#506).
                    if deadline.is_some() && *caught_up != Some(*stage) && !plugin.ready(Instant::now()) {
                        *behind = Some(*stage);
                        *waiting = Some(*stage);
                        return true;
                    }
                    let last = (to - from).saturating_sub(1);
                    span_events.clear();
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    let Some(span) = buffer.get_mut(from * 2..to * 2) else {
                        *stage += 1;
                        continue;
                    };
                    let (sent, next) = send_plugins(slots, *stage, (from, to), span, span_events, *deadline, joined);
                    match sent {
                        Sent::Out => {
                            *waiting = Some(*stage);
                            *sent_at = Some(Instant::now());
                            return true;
                        }
                        // A miss already counted: running it again would count it twice.
                        Sent::Refused => {}
                        Sent::Unsupported => {
                            if let Some(Slot::Plugin(plugin)) = slots.get_mut(*stage) {
                                plugin.run(span, span_events, *deadline);
                            }
                        }
                    }
                    *stage = next;
                }
            }
        }
        pending.clear();
        *done = to;
        false
    }

    /// Whether a block [`Chain::advance`] handed over is out, waiting for its answer.
    pub(crate) fn is_waiting(&self) -> bool {
        self.waiting.is_some()
    }

    /// Whether [`Chain::collect`] can take the block out now without waiting: its plug-in has
    /// answered, or the time it is given is over (the block's deadline; offline,
    /// [`OFFLINE_TIMEOUT`] after it went out), and `collect` counts the miss. Waits for the
    /// answer at most until `until` (a time already past only looks). True when no block is
    /// out. For a plug-in that is waited for to finish a block it was late with, whether it has
    /// (or the deadline is over), so [`Chain::advance`] sends it this block. Never allocates.
    pub(crate) fn answered(&mut self, until: Instant) -> bool {
        let Some(index) = self.waiting else { return true };
        let Some(limit) = self.limit() else { return true };
        if Instant::now() >= limit {
            return true;
        }
        let behind = self.behind == Some(index);
        match self.slots.get_mut(index) {
            Some(Slot::Plugin(plugin)) if behind => plugin.ready(until.min(limit)) || Instant::now() >= limit,
            Some(Slot::Plugin(plugin)) => plugin.poll(until.min(limit)) || Instant::now() >= limit,
            _ => true,
        }
    }

    /// Until when the block out may take: the block's deadline, or offline, where there is
    /// none, [`OFFLINE_TIMEOUT`] after it went out.
    fn limit(&self) -> Option<Instant> {
        self.deadline.or_else(|| self.sent_at.and_then(|at| at.checked_add(OFFLINE_TIMEOUT)))
    }

    /// Waits for the block [`Chain::advance`] handed over, if one is out, and takes its result
    /// (a miss when it is not answered in its time, [`Chain::answered`]).
    pub(crate) fn collect(&mut self, frames: usize) {
        let Some(index) = self.waiting.take() else { return };
        // Nothing went out: the plug-in has finished its late block (or the deadline is over),
        // and `advance` hands it this block now (or counts it refused).
        if self.behind == Some(index) {
            self.behind = None;
            self.caught_up = Some(index);
            return;
        }
        let limit = self.limit();
        self.sent_at = None;
        let to = frames.min(MAX_BLOCK);
        let from = self.done.min(to);
        let Self { slots, buffer, span_events, joined, stage, .. } = self;
        *stage = match buffer.get_mut(from * 2..to * 2) {
            Some(span) => collect_plugins(slots, index, span, span_events, limit, joined),
            None => index + 1,
        };
    }

    /// Adds a frame of the track's sound at `frame` of the block.
    pub(crate) fn add(&mut self, frame: usize, left: f32, right: f32) {
        if let Some([l, r]) =
            self.buffer.get_mut(frame * 2..frame * 2 + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok())
        {
            *l += left;
            *r += right;
        }
    }

    /// Plays `key` from `frame` of the block (renderer frame `position`) until renderer frame
    /// `end`. A key that is already held is released first.
    pub(crate) fn note_on(&mut self, frame: usize, position: u64, key: u8, velocity: f32, end: u64) {
        let Some(held) = self.held.get_mut(usize::from(key)) else { return };
        if *held != NOT_HELD {
            *held = NOT_HELD;
            self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
        }
        // A note of velocity 0 would be a release.
        let velocity = velocity.clamp(1.0 / 127.0, 1.0);
        self.push(frame, NoteEvent::On { key, velocity });
        if let Some(held) = self.held.get_mut(usize::from(key)) {
            *held = end.max(position + 1);
        }
        self.next_off = self.next_off.min(end.max(position + 1));
    }

    /// Releases the notes that end by renderer frame `position`, at `frame` of the block.
    pub(crate) fn release_due(&mut self, frame: usize, position: u64) {
        if position < self.next_off {
            return;
        }
        let mut next = NOT_HELD;
        for key in 0..=127u8 {
            let end = self.held.get(usize::from(key)).copied().unwrap_or(NOT_HELD);
            if end == NOT_HELD {
                continue;
            }
            if end <= position {
                if let Some(held) = self.held.get_mut(usize::from(key)) {
                    *held = NOT_HELD;
                }
                self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
            } else {
                next = next.min(end);
            }
        }
        self.next_off = next;
    }

    /// Holds `key`, started by [`Chain::note_on`] or about to start, until it is released by
    /// hand: the piano roll's key column (`preview.noteOn`). `end` is the frame the note would
    /// otherwise end at; `u64::MAX` holds it for good.
    pub(crate) fn hold_key(&mut self, key: u8, end: u64) {
        if let Some(held) = self.held.get_mut(usize::from(key)) {
            *held = end;
        }
    }

    /// Releases `key` now, at `frame` of the block, if it is held.
    pub(crate) fn release_key(&mut self, frame: usize, key: u8) {
        if let Some(held) = self.held.get_mut(usize::from(key))
            && *held != NOT_HELD
        {
            *held = NOT_HELD;
            self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
        }
    }

    /// Releases every note now, at `frame` of the block.
    /// Each held key is released on its own, then everything: an instrument that ignores an
    /// all-notes-off (many plug-ins do) still lets go of its notes.
    pub(crate) fn release_all(&mut self, frame: usize) {
        if !self.instrument {
            return;
        }
        for key in 0..=127u8 {
            if self.held.get(usize::from(key)).is_some_and(|&end| end != NOT_HELD) {
                self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
            }
        }
        self.held = [NOT_HELD; 128];
        self.next_off = NOT_HELD;
        self.push(frame, NoteEvent::AllNotesOff);
    }

    fn push(&mut self, frame: usize, event: NoteEvent) {
        if self.pending.len() >= MAX_PENDING {
            // Too many events in one block: run the devices up to here first.
            self.run(frame);
        }
        if self.pending.len() < MAX_PENDING {
            self.pending.push((frame.max(self.done), event));
        }
    }

    /// Runs the devices over the block up to `frame`, one after another, handing each queued
    /// event over at its frame.
    fn run(&mut self, frame: usize) {
        let to = frame.min(MAX_BLOCK);
        let Self {
            slots,
            buffer,
            pending,
            span_events,
            midi_scratch,
            done,
            deadline,
            context,
            device_lanes,
            joined,
            ..
        } = self;
        let from = (*done).min(to);
        let mut index = 0;
        while let Some(slot) = slots.get_mut(index) {
            match slot {
                Slot::Device(device) => {
                    run_device(device, index, buffer, from, to, pending, device_lanes, context);
                    index += 1;
                }
                Slot::Sd(effect) => {
                    run_sd_effect(effect, index, buffer, device_lanes, SdRun { from, to, context });
                    index += 1;
                }
                Slot::InstrumentSd(instrument) => {
                    span_events.clear();
                    let last = (to - from).saturating_sub(1);
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    run_sd_instrument(
                        instrument,
                        index,
                        buffer,
                        span_events,
                        midi_scratch,
                        device_lanes,
                        SdRun { from, to, context },
                    );
                    index += 1;
                }
                Slot::MidiSd(effect) => {
                    run_sd_midi(effect, index, pending, midi_scratch, device_lanes, SdRun { from, to, context });
                    std::mem::swap(pending, midi_scratch);
                    index += 1;
                }
                Slot::Plugin(plugin) => {
                    // A plug-in still at a block it was late with gets this one as soon as it is
                    // done, within the deadline (#506). A span with nothing left to run (a chain
                    // `advance` has finished) sends nothing, so nothing is waited for.
                    if let Some(deadline) = *deadline
                        && from < to
                    {
                        plugin.ready(deadline);
                    }
                    let last = (to - from).saturating_sub(1);
                    span_events.clear();
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    let Some(span) = buffer.get_mut(from * 2..to * 2) else {
                        index += 1;
                        continue;
                    };
                    let (sent, next) = send_plugins(slots, index, (from, to), span, span_events, *deadline, joined);
                    index = match sent {
                        Sent::Out => collect_plugins(slots, index, span, span_events, *deadline, joined),
                        Sent::Refused => next,
                        Sent::Unsupported => {
                            if let Some(Slot::Plugin(plugin)) = slots.get_mut(index) {
                                plugin.run(span, span_events, *deadline);
                            }
                            next
                        }
                    };
                }
            }
        }
        pending.clear();
        *done = to;
    }

    /// Runs the devices over the rest of a block of `frames` frames; [`Chain::mix`] then adds
    /// the result to where the track plays into, and the sends read it before that.
    pub(crate) fn complete(&mut self, frames: usize, context: &ProcessContext) {
        self.context = *context;
        self.run(frames);
    }

    /// Adds what the devices made of the block, through the latency compensation and at the
    /// track's gains, to `out`, and, when its cue is on, at its cue gains to `cue`.
    pub(crate) fn mix(&mut self, out: &mut [f32], cue: &mut [f32], frames: usize) {
        let (gain_left, gain_right) = self.post;
        let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) else { return };
        self.delay.run(buffer);
        let pairs = out.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0);
        match &self.mix {
            None => {
                for ([l, r], [cl, cr]) in pairs {
                    *l += cl * gain_left;
                    *r += cr * gain_right;
                }
            }
            Some(mix) => {
                for (frame, ([l, r], [cl, cr])) in pairs.enumerate() {
                    let (left, right) = mix.gains(frame);
                    *l += cl * gain_left * left;
                    *r += cr * gain_right * right;
                }
            }
        }
        if let Some((cue_left, cue_right)) = self.cue {
            for ([l, r], [cl, cr]) in cue.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0) {
                *l += cl * cue_left;
                *r += cr * cue_right;
            }
        }
    }

    /// The first `frames` frames of the block's output, before the track's gains, once
    /// [`Chain::complete`] has run.
    pub(crate) fn output(&self, frames: usize) -> &[f32] {
        let end = (frames.min(MAX_BLOCK) * 2).min(self.buffer.len());
        self.buffer.get(..end).unwrap_or_default()
    }

    /// Sets parameter `param` of the device at `position` among the channel's devices, as a
    /// parameter message does (ADR 0005, amendment). Never allocates.
    /// For a plug-in, `param` is its parameter id, and the value goes at the start of the next
    /// block.
    pub(crate) fn set_param(&mut self, position: usize, param: usize, value: f64) {
        let Some(index) = self.positions.iter().position(|&p| p == position) else { return };
        match self.slots.get_mut(index) {
            Some(Slot::Device(device)) => {
                device.set_param(param, value);
                if let Some(slot) = self.params.get_mut(index).and_then(|values| values.get_mut(param)) {
                    *slot = device.param(param).unwrap_or(value);
                }
            }
            Some(slot @ (Slot::Sd(_) | Slot::InstrumentSd(_) | Slot::MidiSd(_))) => {
                slot.set_sd_parameter(param, value);
                if let Some(slot) = self.params.get_mut(index).and_then(|values| values.get_mut(param)) {
                    *slot = value;
                }
            }
            Some(Slot::Plugin(plugin)) => {
                if let Ok(id) = u32::try_from(param) {
                    plugin.params.set(id, value);
                }
            }
            None => {}
        }
    }

    /// Takes the running devices of `old` if it holds the same devices for the same track,
    /// with this chain's parameter values and plug-in settings and `old`'s held notes. Never
    /// allocates.
    pub(crate) fn adopt(&mut self, old: &mut Chain) -> bool {
        if old.track != self.track || old.ids != self.ids {
            return false;
        }
        std::mem::swap(&mut self.slots, &mut old.slots);
        let lanes = &self.device_lanes;
        for (position, ((slot, values), fresh)) in
            self.slots.iter_mut().zip(&self.params).zip(old.slots.iter_mut()).enumerate()
        {
            match (slot, fresh) {
                (Slot::Device(device), _) => {
                    if let Some(points) = self.sample_slices.get_mut(position) {
                        device.swap_sample_slices(points);
                    }
                    // A parameter that follows a lane keeps its running value until the lane
                    // sets it in the next block.
                    for (index, &value) in
                        values.iter().enumerate().filter(|&(index, _)| !lanes.covers(position, index))
                    {
                        device.set_param(index, value);
                    }
                }
                (slot @ (Slot::Sd(_) | Slot::InstrumentSd(_) | Slot::MidiSd(_)), _) => {
                    for (index, &value) in
                        values.iter().enumerate().filter(|&(index, _)| !lanes.covers(position, index))
                    {
                        slot.set_sd_parameter(index, value);
                    }
                }
                (Slot::Plugin(running), Slot::Plugin(fresh)) => {
                    running.enabled = fresh.enabled;
                    running.instrument = fresh.instrument;
                    running.latency = fresh.latency;
                    running.params.adopt(&mut fresh.params);
                    // A processor that reached the new graph's slot (a restart) wins, and gets
                    // the set values again.
                    if fresh.processor.is_some() {
                        std::mem::swap(&mut running.processor, &mut fresh.processor);
                        running.params.restart();
                    }
                }
                (Slot::Plugin(_), _) => {}
            }
        }
        self.held = old.held;
        self.next_off = old.next_off;
        self.delay.adopt(&mut old.delay);
        self.adopt_sends(old);
        true
    }

    /// The plug-in slots, for processors to find theirs.
    pub(crate) fn plugin_slots(&mut self) -> impl Iterator<Item = &mut PluginSlot> {
        self.slots.iter_mut().filter_map(|slot| match slot {
            Slot::Plugin(plugin) => Some(plugin),
            Slot::Device(_) => None,
            Slot::Sd(_) => None,
            Slot::InstrumentSd(_) => None,
            Slot::MidiSd(_) => None,
        })
    }

    /// Puts `processor` into the slot of plug-in `key`, if this chain has it, and returns the
    /// one it replaces; gives `processor` back when there is no such slot. Never allocates.
    pub(crate) fn place(&mut self, key: u64, processor: Box<dyn PluginProcessor>) -> Placed {
        match self.plugin_slots().find(|slot| slot.key == key) {
            Some(slot) => {
                slot.params.restart();
                Placed::Replaced(slot.processor.replace(processor))
            }
            None => Placed::NoSlot(processor),
        }
    }

    /// Blocks its plug-ins did not answer in time, for tests.
    #[cfg(test)]
    pub(crate) fn missed(&mut self) -> u64 {
        self.plugin_slots().map(|slot| slot.missed).sum()
    }
}

/// How long the engine sleeps on one chain's plug-in when none of the blocks out has been
/// answered, before it looks at all of them again (see [`run_as_answered`]): an answer from
/// that plug-in wakes it at once, one from another is seen at most this late.
pub(crate) const ANSWER_SLICE: Duration = Duration::from_micros(50);

/// Takes each chain's block as soon as its plug-in answers and runs that chain on to its next
/// plug-in, until no chain has a block out (#151): a chain does not wait for the plug-ins of the
/// other chains, only for its own, and only until the block's deadline. The chains were started
/// with [`Chain::advance`]. While nothing has answered, it waits for the first chain still out,
/// for at most [`ANSWER_SLICE`] at a time. Never allocates; returns by the deadline in real time
/// (a plug-in still out then is a miss), and offline once every plug-in has answered or used up
/// [`OFFLINE_TIMEOUT`].
pub(crate) fn run_as_answered(chains: &mut [Chain], frames: usize, context: &ProcessContext) {
    loop {
        let now = Instant::now();
        let mut out = false;
        let mut moved = false;
        for chain in chains.iter_mut().filter(|chain| chain.is_waiting()) {
            if chain.answered(now) {
                chain.collect(frames);
                chain.advance(frames, context);
                moved = true;
            }
            out |= chain.is_waiting();
        }
        if !out {
            return;
        }
        if !moved && let Some(chain) = chains.iter_mut().find(|chain| chain.is_waiting()) {
            chain.answered(now + ANSWER_SLICE);
        }
    }
}

/// The plug-ins that run after another in its joint block, by slot index.
#[derive(Default)]
struct Joined {
    indices: [usize; MAX_JOINED],
    len: usize,
}

impl Joined {
    fn indices(&self) -> &[usize] {
        self.indices.get(..self.len).unwrap_or_default()
    }
}

/// Finds the plug-ins that can run after the one at `at` in the same round trip: the enabled
/// plug-ins after it in the same host process, up to the first built-in device or plug-in that
/// runs elsewhere or cannot take part now (disabled plug-ins pass the sound and are passed
/// over). Their slot indices go to `joined`, their numbers in the process to `members`.
///
/// The plug-in at `at` is handed its parameter events for the block's frames `from..to` first
/// ([`PluginSlot::automate`]), and so is each follower (none). A plug-in that has any runs on
/// its own and ends the run (a follower gets them at its own turn): a joint block carries one
/// event list, the first plug-in's, which every plug-in in it is given, so parameter values of
/// one plug-in would reach the others too, or not reach a follower at all.
fn find_joined(
    slots: &mut [Slot],
    at: usize,
    (from, to): (usize, usize),
    joined: &mut Joined,
    members: &mut [u32; MAX_JOINED],
) {
    joined.len = 0;
    let Some(lead) = (match slots.get_mut(at) {
        Some(Slot::Plugin(first)) => {
            let automated = first.automate(from, to);
            first.joint().filter(|_| !automated)
        }
        _ => None,
    }) else {
        return;
    };
    for (index, slot) in slots.iter_mut().enumerate().skip(at + 1) {
        let Slot::Plugin(plugin) = slot else { break };
        if !plugin.enabled {
            continue;
        }
        if plugin.has_params(from, to) {
            break;
        }
        let Some(joint) = plugin.joint().filter(|j| j.process == lead.process) else { break };
        let (Some(at_index), Some(member)) = (joined.indices.get_mut(joined.len), members.get_mut(joined.len)) else {
            break;
        };
        plugin.automate(from, to);
        *at_index = index;
        *member = joint.member;
        joined.len += 1;
    }
}

/// Hands `span` (the block's frames `from..to`) with `events` to the plug-in at `at`, together
/// with the plug-ins after it that run in the same host process when there are any, each with
/// its parameter events for the span ([`find_joined`]). Returns what became of it and the slot to
/// go on with: [`Sent::Out`], wait with [`collect_plugins`]; [`Sent::Refused`], a miss already
/// counted for each (the sound passes, an instrument is silent); [`Sent::Unsupported`], nothing
/// was sent and the plug-in at `at` runs on its own. Never allocates.
fn send_plugins(
    slots: &mut [Slot],
    at: usize,
    from_to: (usize, usize),
    span: &mut [f32],
    events: &[(usize, NoteEvent)],
    deadline: Option<Instant>,
    joined: &mut Joined,
) -> (Sent, usize) {
    let mut members = [0u32; MAX_JOINED];
    find_joined(slots, at, from_to, joined, &mut members);
    if joined.len > 0
        && let Some(Slot::Plugin(first)) = slots.get_mut(at)
    {
        let followers = members.get(..joined.len).unwrap_or_default();
        match first.send_joint(span, events, followers, deadline) {
            Sent::Out => return (Sent::Out, at + 1),
            Sent::Refused => {
                let next = joined.indices().last().map_or(at + 1, |last| last + 1);
                for &index in joined.indices() {
                    if let Some(Slot::Plugin(plugin)) = slots.get_mut(index) {
                        plugin.followed(span, events, Followed::Refused);
                    }
                }
                joined.len = 0;
                return (Sent::Refused, next);
            }
            Sent::Unsupported => joined.len = 0,
        }
    }
    joined.len = 0;
    match slots.get_mut(at) {
        Some(Slot::Plugin(plugin)) => (plugin.send(span, events, deadline), at + 1),
        _ => (Sent::Unsupported, at + 1),
    }
}

/// Waits for the block [`send_plugins`] handed to the plug-in at `at`, puts the result in
/// `span` and tells the plug-ins that followed it in the block how it went. Returns the slot
/// to go on with. Never allocates.
fn collect_plugins(
    slots: &mut [Slot],
    at: usize,
    span: &mut [f32],
    events: &[(usize, NoteEvent)],
    deadline: Option<Instant>,
    joined: &mut Joined,
) -> usize {
    let answered = match slots.get_mut(at) {
        Some(Slot::Plugin(plugin)) => plugin.finish(span, deadline),
        _ => false,
    };
    let how = if answered { Followed::Answered } else { Followed::Late };
    let next = joined.indices().last().map_or(at + 1, |last| last + 1);
    for &index in joined.indices() {
        if let Some(Slot::Plugin(plugin)) = slots.get_mut(index) {
            plugin.followed(span, events, how);
        }
    }
    joined.len = 0;
    next
}

/// What [`Chain::place`] did with a processor.
pub(crate) enum Placed {
    /// It went into its slot; the processor that was there, if one was.
    Replaced(Option<Box<dyn PluginProcessor>>),
    /// The chain has no slot for it; here it is back.
    NoSlot(Box<dyn PluginProcessor>),
}

/// Runs a built-in device (slot `slot` of its chain) over the block's frames `from..to`,
/// split where a note event or a change of a parameter that follows a lane falls; at one frame
/// the parameter changes before the note.
#[allow(clippy::too_many_arguments)]
fn run_device(
    device: &mut Box<dyn Device>,
    slot: usize,
    buffer: &mut [f32],
    from: usize,
    to: usize,
    pending: &[(usize, NoteEvent)],
    lanes: &DeviceLanes,
    context: &ProcessContext,
) {
    // Give context-dependent instruments the tempo before an event at frame zero.
    device.set_context(context);
    let mut at_done = from;
    let mut changes = lanes.changes(slot, from, to).peekable();
    for &(at, event) in pending {
        let at = at.clamp(at_done, to);
        while let Some(change) = changes.next_if(|c| c.frame <= at) {
            process(device, buffer, at_done, change.frame.max(at_done), context);
            at_done = change.frame.max(at_done);
            device.set_param(change.param, change.value);
        }
        process(device, buffer, at_done, at, context);
        at_done = at;
        device.event(event);
    }
    for change in changes {
        process(device, buffer, at_done, change.frame.max(at_done), context);
        at_done = change.frame.max(at_done);
        device.set_param(change.param, change.value);
    }
    process(device, buffer, at_done, to, context);
}

#[derive(Clone, Copy)]
struct SdRun<'a> {
    from: usize,
    to: usize,
    context: &'a ProcessContext,
}

fn run_sd_effect(
    effect: &mut suhina_device_sdk::PreparedEffect,
    slot: usize,
    buffer: &mut [f32],
    lanes: &DeviceLanes,
    run: SdRun<'_>,
) {
    let mut at = run.from;
    for change in lanes.changes(slot, run.from, run.to) {
        let end = change.frame.max(at).min(run.to);
        if let Some(span) = buffer.get_mut(at * 2..end * 2) {
            effect.process_context(&sd_context(run.context, at, effect.sample_rate()), span);
        }
        effect.set_parameter(change.param, change.value);
        at = end;
    }
    if let Some(span) = buffer.get_mut(at * 2..run.to * 2) {
        effect.process_context(&sd_context(run.context, at, effect.sample_rate()), span);
    }
}

fn run_sd_instrument(
    instrument: &mut suhina_device_sdk::PreparedEffect,
    slot: usize,
    buffer: &mut [f32],
    events: &[(usize, NoteEvent)],
    scratch: &mut Vec<(usize, NoteEvent)>,
    lanes: &DeviceLanes,
    run: SdRun<'_>,
) {
    let mut at = run.from;
    for change in lanes.changes(slot, run.from, run.to) {
        let end = change.frame.max(at).min(run.to);
        run_sd_instrument_span(instrument, buffer, at, end, events, scratch, run);
        instrument.set_parameter(change.param, change.value);
        at = end;
    }
    run_sd_instrument_span(instrument, buffer, at, run.to, events, scratch, run);
}

fn run_sd_instrument_span(
    instrument: &mut suhina_device_sdk::PreparedEffect,
    buffer: &mut [f32],
    start: usize,
    end: usize,
    events: &[(usize, NoteEvent)],
    scratch: &mut Vec<(usize, NoteEvent)>,
    run: SdRun<'_>,
) {
    if end <= start {
        return;
    }
    scratch.clear();
    scratch.extend(events.iter().filter_map(|&(frame, event)| {
        (frame >= start.saturating_sub(run.from) && frame < end.saturating_sub(run.from))
            .then_some((frame.saturating_sub(start.saturating_sub(run.from)), event))
    }));
    if let Some(span) = buffer.get_mut(start * 2..end * 2) {
        instrument.process_instrument_context(&sd_context(run.context, start, instrument.sample_rate()), scratch, span);
    }
}

fn run_sd_midi(
    effect: &mut suhina_device_sdk::PreparedMidiEffect,
    slot: usize,
    events: &[(usize, NoteEvent)],
    output: &mut Vec<(usize, NoteEvent)>,
    lanes: &DeviceLanes,
    run: SdRun<'_>,
) {
    output.resize(events.len().saturating_mul(2).min(output.capacity()), (0, NoteEvent::AllNotesOff));
    let mut written = 0;
    let mut changes = lanes.changes(slot, run.from, run.to).peekable();
    for event in events {
        while let Some(change) = changes.next_if(|change| change.frame <= event.0) {
            effect.set_parameter(change.param, change.value);
        }
        if let Some(dest) = output.get_mut(written..) {
            written += effect.process(std::slice::from_ref(event), dest);
        }
    }
    for change in changes {
        effect.set_parameter(change.param, change.value);
    }
    output.truncate(written);
}

fn sd_context(context: &ProcessContext, frame: usize, sample_rate: f64) -> ProcessContext {
    let mut context = *context;
    if context.playing {
        context.beat += frame as f64 * context.tempo / (60.0 * sample_rate);
    }
    context
}

fn process(device: &mut Box<dyn Device>, buffer: &mut [f32], from: usize, to: usize, context: &ProcessContext) {
    if to <= from {
        return;
    }
    let Some(span) = buffer.get_mut(from * 2..to * 2) else { return };
    device.process(context, span);
}

#[cfg(test)]
#[path = "chain_wait_tests.rs"]
mod wait_tests;

#[cfg(test)]
mod sd_tests {
    use super::*;

    #[test]
    fn midi_effect_preserves_block_offsets_after_a_partial_flush() {
        use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind};
        let mut graph = DeviceGraph::new("test.transpose", "Transpose", DeviceRole::MidiEffect);
        graph.nodes = vec![
            Node::new("in", NodeKind::MidiInput),
            Node::new("transpose", NodeKind::MidiTranspose { semitones: 12 }),
            Node::new("out", NodeKind::MidiOutput),
        ];
        graph.connections = vec![Connection::new("in", "transpose"), Connection::new("transpose", "out")];
        let mut effect = graph.compile_midi().unwrap();
        let events = [(150, NoteEvent::On { key: 60, velocity: 0.8 })];
        let mut output = Vec::with_capacity(1);
        run_sd_midi(
            &mut effect,
            0,
            &events,
            &mut output,
            &DeviceLanes::none(),
            SdRun { from: 100, to: 200, context: &ProcessContext::default() },
        );
        assert_eq!(output, [(150, NoteEvent::On { key: 72, velocity: 0.8 })]);
    }
}

#[cfg(test)]
mod sampler_regressions {
    use super::*;
    #[test]
    fn first_beat_slice_uses_context_before_frame_zero_note() {
        let mut sampler = suhina_device_registry::create("sampler").unwrap();
        let source =
            std::sync::Arc::new(crate::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.5; 96000] });
        sampler.load_shared_sample(48000, 1, source.clone());
        assert_eq!(std::sync::Arc::strong_count(&source), 2, "PCM must be shared");
        for (id, value) in [("playback", 3.), ("slice_by", 1.), ("sensitivity", 0.4), ("attack", 0.), ("sustain", 1.)] {
            sampler.set_param(sampler.info().param_index(id).unwrap(), value);
        }
        let mut context = ProcessContext { tempo: 60., ..ProcessContext::default() };
        let mut buffer = vec![0.; 2048];
        run_device(
            &mut sampler,
            0,
            &mut buffer,
            0,
            1024,
            &[(0, NoteEvent::On { key: 60, velocity: 1. })],
            &DeviceLanes::none(),
            &context,
        );
        for _ in 0..28 {
            run_device(&mut sampler, 0, &mut buffer, 0, 1024, &[], &DeviceLanes::none(), &context);
        }
        assert!(buffer.iter().any(|s| *s > 0.1), "one-beat slice at 60 BPM must outlast half a second");
        context.tempo = 120.;
        sampler.reset();
        run_device(
            &mut sampler,
            0,
            &mut buffer,
            0,
            1024,
            &[(0, NoteEvent::On { key: 60, velocity: 1. })],
            &DeviceLanes::none(),
            &context,
        );
        for _ in 0..28 {
            run_device(&mut sampler, 0, &mut buffer, 0, 1024, &[], &DeviceLanes::none(), &context);
        }
        assert!(buffer.iter().all(|s| *s == 0.), "one-beat slice at 120 BPM ends after half a second");
    }
}
