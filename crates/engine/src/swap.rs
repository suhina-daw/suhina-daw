// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Edits reaching a playing renderer (ADR 0005).
//!
//! [`Controller::update`] builds a new [`Graph`] for the edited project on the calling thread,
//! picks a frame a little ahead of the playhead, moves the graph's stretch voices there and sends
//! the graph to the audio thread. [`Renderer::process`] splits its block at that frame, carries
//! its state over to the new graph and sends the old one back, so nothing is allocated or freed
//! on the audio thread. Every graph whose frame falls in a block is taken in that block.
//!
//! A device parameter does not need a new graph: [`Controller::set_device_param`] sends it as a
//! message, applied at the start of the next block. Messages are numbered and every graph
//! records the last number it already contains, so a graph built before a message (still on
//! its way to the audio thread) gets the message again when it takes over.
//!
//! A running plug-in reaches the audio thread the same way, as a message
//! ([`Controller::send_plugin`]): its processor goes into its slot in the playing graph, or
//! waits until a graph with that slot takes over, and every later graph takes it over from the
//! one before (ADR 0006). The processor it replaces, and those of slots that are gone, go back
//! to be dropped off the audio thread.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rtrb::{Consumer, Producer, RingBuffer};
use suhina_model::{Project, TrackId};

use crate::ahead::StretchThread;
use crate::audio::AudioPool;
use crate::chain::Placed;
use crate::launcher::{Controller, SceneShift};
use crate::plugin::{PluginProcessor, PluginSlotInfo, Plugins};
use crate::{Graph, MAX_AUDIO_VOICES, Renderer};

/// How far ahead of the playhead a new graph with stretched clips takes over, in seconds: time
/// to move its stretch voices and fill their rings before the audio thread reaches it. A graph
/// without stretched clips takes over at the next block.
const SWAP_AHEAD_SECONDS: f64 = 0.15;
/// How far, in frames, a swap may land from where its stretch voices were rendered for and
/// still play them as they are.
const SWAP_TOLERANCE: u64 = 1024;
/// Graphs that can wait in either direction.
const QUEUE: usize = 16;
/// Parameter messages that can wait for the audio thread.
const PARAM_QUEUE: usize = 1024;
/// Parameter messages the audio thread remembers, to apply again to a graph built before them.
const RECENT_PARAMS: usize = 256;
/// Plug-in processors that can wait in either direction, and wait for their slot.
const PLUGIN_QUEUE: usize = 64;

type Processor = Box<dyn PluginProcessor>;

/// A new value for one parameter of a device on a track (ADR 0005, amendment).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParamChange {
    seq: u64,
    track: TrackId,
    /// The device's index among the track channel's devices in the project.
    device: usize,
    param: usize,
    value: f64,
}

/// A graph and the renderer frame it takes over at.
pub(crate) struct Swap {
    graph: Box<Graph>,
    at: u64,
    /// The last parameter message the graph's project already holds.
    param_seq: u64,
    /// The arrangement frame the graph's stretch voices were moved to for `at`, or `None` if the
    /// transport was stopped (they are at the start).
    arrangement: Option<u64>,
    /// How the graph's scenes line up with the graph before it, when a scene was inserted or
    /// removed.
    scenes: Option<SceneShift>,
}

/// The renderer's end of the swap queues.
#[derive(Default)]
pub(crate) struct SwapState {
    incoming: Option<Consumer<Swap>>,
    retired: Option<Producer<Box<Graph>>>,
    pending: Option<Swap>,
    /// An old graph the controller has not taken back yet; no new graph is taken until it has.
    held: Option<Box<Graph>>,
    params: Option<Consumer<ParamChange>>,
    /// The last parameter messages, oldest overwritten first.
    recent: Vec<ParamChange>,
    recent_next: usize,
    plugins: Option<Consumer<(u64, Processor)>>,
    retired_plugins: Option<Producer<Processor>>,
    /// Processors whose slot no graph has had yet; allocated with room for `PLUGIN_QUEUE`.
    parked: Vec<(u64, Processor)>,
}

/// The controller's end: builds graphs and takes old ones back to free them.
pub(crate) struct Updater {
    swaps: Producer<Swap>,
    retired: Consumer<Box<Graph>>,
    audio: Arc<AudioPool>,
    /// The file-backed devices' files ([docs/spec/nam-amp-and-cab.md]), for every graph built.
    assets: Arc<crate::DeviceAssets>,
    sample_rate: u32,
    stretch: Option<StretchThread>,
    params: Producer<ParamChange>,
    param_seq: u64,
    /// What is known about the project's plug-in devices; every graph is built with it.
    plugin_info: HashMap<u64, PluginSlotInfo>,
    plugins: Producer<(u64, Processor)>,
    retired_plugins: Consumer<Processor>,
    /// The tempo map of the last graph sent, which plays once the swaps before it are done.
    tempo: Arc<suhina_model::TempoMap>,
    /// The tracks whose chain in the last graph sent takes a live volume.
    live_volumes: Vec<TrackId>,
    /// The sends (track, return) a knob's turn reaches without a new graph.
    live_sends: Vec<(TrackId, TrackId)>,
    /// The tracks whose pan a knob's turn reaches without a new graph.
    live_pans: Vec<TrackId>,
}

impl Updater {
    fn collect(&mut self) {
        while let Ok(graph) = self.retired.pop() {
            drop(graph);
        }
        while let Ok(processor) = self.retired_plugins.pop() {
            drop(processor);
        }
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.collect();
    }
}

impl Renderer {
    /// Connects a new pair of swap queues; the controller gets the other ends.
    pub(crate) fn updater(&mut self) -> Updater {
        let (swaps, incoming) = RingBuffer::new(QUEUE);
        let (retired_producer, retired) = RingBuffer::new(QUEUE);
        let (params, param_consumer) = RingBuffer::new(PARAM_QUEUE);
        let (plugins, plugin_consumer) = RingBuffer::new(PLUGIN_QUEUE);
        let (retired_plugins_producer, retired_plugins) = RingBuffer::new(PLUGIN_QUEUE);
        self.swap.incoming = Some(incoming);
        self.swap.retired = Some(retired_producer);
        self.swap.params = Some(param_consumer);
        self.swap.recent = Vec::with_capacity(RECENT_PARAMS);
        self.swap.recent_next = 0;
        self.swap.plugins = Some(plugin_consumer);
        self.swap.retired_plugins = Some(retired_plugins_producer);
        self.swap.parked = Vec::with_capacity(PLUGIN_QUEUE);
        Updater {
            swaps,
            retired,
            audio: Arc::clone(&self.audio),
            assets: Arc::clone(&self.assets),
            sample_rate: self.sample_rate,
            stretch: None,
            params,
            param_seq: 0,
            plugin_info: self.plugin_info.clone(),
            plugins,
            retired_plugins,
            tempo: Arc::clone(&self.graph.tempo),
            live_volumes: self.graph.chains.iter().filter(|c| c.built_volume.is_some()).map(|c| c.track).collect(),
            live_sends: live_sends(&self.graph.chains),
            live_pans: self.graph.chains.iter().filter(|c| c.built_pan.is_some()).map(|c| c.track).collect(),
        }
    }

    /// Takes the plug-in processors that have arrived: each goes into its slot in the playing
    /// graph (the one it replaces goes back), or waits for a graph that has its slot. Never
    /// allocates: the waiting list was allocated with its full capacity, and a processor is
    /// taken only when there is room for whatever it displaces.
    pub(crate) fn receive_plugins(&mut self) {
        let swap = &mut self.swap;
        let (Some(incoming), Some(retired)) = (swap.plugins.as_mut(), swap.retired_plugins.as_mut()) else { return };
        while retired.slots() > 0 && swap.parked.len() < swap.parked.capacity() {
            let Ok((key, processor)) = incoming.pop() else { break };
            match place(&mut self.graph.chains, key, processor) {
                Placed::Replaced(Some(old)) => {
                    let _full = retired.push(old);
                }
                Placed::Replaced(None) => {}
                Placed::NoSlot(processor) => park(&mut swap.parked, retired, key, processor),
            }
        }
    }

    /// Takes the next graph from the queue, once any old graph has gone back. Never blocks.
    pub(crate) fn receive_swap(&mut self) {
        if let Some(old) = self.swap.held.take()
            && let Some(retired) = self.swap.retired.as_mut()
            && let Err(rtrb::PushError::Full(old)) = retired.push(old)
        {
            self.swap.held = Some(old);
        }
        if self.swap.held.is_none()
            && self.swap.pending.is_none()
            && let Some(incoming) = self.swap.incoming.as_mut()
        {
            self.swap.pending = incoming.pop().ok();
        }
    }

    /// Applies the parameter messages that have arrived. Never allocates: the list of recent
    /// messages was allocated with its full capacity.
    pub(crate) fn receive_params(&mut self) {
        let Some(consumer) = self.swap.params.as_mut() else { return };
        while let Ok(change) = consumer.pop() {
            apply_param(&mut self.graph.chains, change);
            let recent = &mut self.swap.recent;
            if recent.len() < RECENT_PARAMS {
                recent.push(change);
            } else if let Some(slot) = recent.get_mut(self.swap.recent_next) {
                *slot = change;
            }
            self.swap.recent_next = (self.swap.recent_next + 1) % RECENT_PARAMS;
        }
    }

    /// The frame the pending graph takes over at, if one is waiting.
    pub(crate) fn swap_at(&self) -> Option<u64> {
        self.swap.pending.as_ref().map(|s| s.at)
    }

    /// Swaps in the pending graph and carries the state over (ADR 0005). Never allocates.
    pub(crate) fn apply_swap(&mut self) {
        let Some(swap) = self.swap.pending.take() else { return };
        let old_time = self.time();
        let param_seq = swap.param_seq;
        let mut old = std::mem::replace(&mut self.graph, swap.graph);
        // The playhead stays on the same beat when the tempo map changed.
        if self.running && *old.tempo != *self.graph.tempo {
            self.time_at_origin = crate::tempo::rescale(old_time, &old.tempo, &self.graph.tempo);
            self.origin = self.position;
        }
        let was_on_map = old.launcher.follows_map();
        self.graph.launcher.adopt(&mut old.launcher, swap.scenes);
        // An edited tempo map takes the clock back from a scene's tempo, to the playhead's beat.
        if !was_on_map && self.graph.launcher.follows_map() {
            self.clock_to_map();
        }
        // A chain with the same devices on the same track keeps them running, so held notes,
        // envelopes and tails carry on; the new parameter values apply (ADR 0004).
        for chain in &mut self.graph.chains {
            for old_chain in &mut old.chains {
                if chain.adopt(old_chain) {
                    break;
                }
            }
        }
        // Running plug-ins carry over to their slots in the new graph wherever the chain around
        // them changed, and waiting ones take theirs if it has them now.
        for chain in &mut self.graph.chains {
            for slot in chain.plugin_slots().filter(|s| s.processor.is_none()) {
                slot.processor = old
                    .chains
                    .iter_mut()
                    .flat_map(|c| c.plugin_slots())
                    .find(|o| o.key == slot.key && o.processor.is_some())
                    .and_then(|o| o.processor.take());
                if slot.processor.is_none()
                    && let Some(index) = self.swap.parked.iter().position(|(key, _)| *key == slot.key)
                {
                    slot.processor = Some(self.swap.parked.swap_remove(index).1);
                }
            }
        }
        self.graph.direct_delay.adopt(&mut old.direct_delay);
        self.meters.assign(self.graph.levels.tracks());
        // The new graph's chains capture their watched devices' output into the meter
        // monitor's spectrum rings, on every path that runs them
        // (docs/spec/eq.md › The panel › The analyser).
        for chain in &mut self.graph.chains {
            chain.spectrum = Some(Arc::clone(&self.meters.monitor.spectrum));
        }
        // Parameter messages newer than the new graph's project apply to it again, in order.
        let mut newer = [usize::MAX; RECENT_PARAMS];
        let mut count = 0;
        for (index, change) in self.swap.recent.iter().enumerate() {
            if change.seq > param_seq
                && let Some(slot) = newer.get_mut(count)
            {
                *slot = index;
                count += 1;
            }
        }
        let order = newer.get_mut(..count).unwrap_or_default();
        order.sort_unstable_by_key(|&i| self.swap.recent.get(i).map_or(0, |c| c.seq));
        for &i in order.iter() {
            if let Some(&change) = self.swap.recent.get(i) {
                apply_param(&mut self.graph.chains, change);
            }
        }
        // Sounding voices follow their track: its mix (volume, pan, mute) and its chain in the
        // new graph. A voice whose track is gone ends.
        for voice in self.voices.iter_mut().filter(|v| v.active) {
            match self.graph.routes.iter().position(|(track, _, _)| track.0 == voice.track) {
                Some(index) => {
                    let Some(&(_, bus, (left, right))) = self.graph.routes.get(index) else { continue };
                    voice.bus = bus;
                    // The routes are in the project's track order, as the meter slots are.
                    voice.meter = if index < crate::MAX_METERED_TRACKS {
                        u32::try_from(index).unwrap_or(crate::meters::NO_METER)
                    } else {
                        crate::meters::NO_METER
                    };
                    voice.gain_left = left * voice.velocity;
                    voice.gain_right = right * voice.velocity;
                }
                None => voice.active = false,
            }
        }
        let time = self.time();
        // Cursors into the new lists: what has started by now has been handled.
        self.next_event = self.graph.events.partition_point(|e| e.start < time);
        self.next_audio = self.graph.audio_events.partition_point(|a| a.event.start < time);
        self.active_audio_count = 0;
        if self.running {
            for (index, arranged) in self.graph.audio_events.iter_mut().enumerate().take(self.next_audio) {
                if arranged.event.end > time && self.active_audio_count < MAX_AUDIO_VOICES {
                    if let Some(slot) = self.active_audio.get_mut(self.active_audio_count) {
                        *slot = index;
                    }
                    self.active_audio_count += 1;
                    // An unwarped clip plays at its own speed, not the beat's: it carries on from
                    // the file position it was at, whatever the tempo now.
                    if !arranged.event.warped()
                        && let Some(before) = old
                            .audio_events
                            .iter()
                            .find(|a| a.key == arranged.key && a.event.start <= old_time && old_time < a.event.end)
                    {
                        arranged.event.continue_from(time, before.event.file_seconds(old_time));
                    }
                }
            }
        }
        // The unchanged clips' rings go on in the new graph (#743); the others start afresh,
        // moved to where the playhead was expected.
        self.graph.ahead.adopt_from(&mut old.ahead);
        self.underruns_before += old.ahead.underruns();
        if self.running {
            // A swap a few frames off where it was expected keeps the voices rendered for it: the
            // reader drops the frames already past or waits out the few not yet due. Moving them
            // emptied every stretched clip's ring at each edit, and a large project dropped out
            // while a fader moved.
            let near = swap.arrangement.is_some_and(|expected| expected.abs_diff(time) <= SWAP_TOLERANCE);
            if !near {
                self.graph.ahead.move_to(time);
                self.graph.launcher.monitor.stretch_moves.fetch_add(1, Ordering::Relaxed);
            }
            self.stretch_fresh = false;
        } else {
            // The new graph's voices are at the start; the next start is at the marker.
            let start = self.start_frame();
            if swap.arrangement.is_some() || start > 0 {
                self.graph.ahead.move_to(start);
            }
            self.stretch_fresh = true;
        }
        self.swap.held = Some(old);
        self.receive_swap();
    }

    /// Publishes the playhead for the controller.
    pub(crate) fn publish_position(&self) {
        let monitor = &self.graph.launcher.monitor;
        monitor.position.store(self.position, Ordering::Relaxed);
        monitor.arrangement.store(self.time(), Ordering::Relaxed);
        monitor.stretch_underruns.store(self.stretch_underruns(), Ordering::Relaxed);
        monitor.frames_per_beat.store(self.graph.tempo.start_per_beat().to_bits(), Ordering::Relaxed);
        monitor.start_beat.store(self.start_beat.to_bits(), Ordering::Relaxed);
    }
}

/// Puts `processor` into the slot of plug-in `key` among `chains`.
fn place(chains: &mut [crate::chain::Chain], key: u64, mut processor: Processor) -> Placed {
    for chain in chains.iter_mut() {
        match chain.place(key, processor) {
            Placed::NoSlot(back) => processor = back,
            placed => return placed,
        }
    }
    Placed::NoSlot(processor)
}

/// Keeps `processor` until a graph with its slot takes over; one for the same plug-in that was
/// already waiting goes back. Never allocates: the caller checked there is room.
/// What a renderer gives back of its plug-ins: each device's processor, and what its slot
/// knew about it (instrument, latency).
#[derive(Default)]
pub(crate) struct Taken {
    pub(crate) processors: HashMap<u64, Processor>,
    pub(crate) info: HashMap<u64, PluginSlotInfo>,
}

impl Taken {
    /// Takes every plug-in processor out of `graph`'s chains, and what each slot knows.
    fn take_graph(&mut self, graph: &mut Graph) {
        for chain in &mut graph.chains {
            for slot in chain.plugin_slots() {
                self.info.insert(slot.key, PluginSlotInfo { instrument: slot.instrument, latency: slot.latency });
                if let Some(processor) = slot.processor.take() {
                    self.processors.insert(slot.key, processor);
                }
            }
        }
    }
}

impl SwapState {
    /// Takes every processor the renderer holds: in the graph not taken back yet, the
    /// `current` one, parked without a slot, in graphs waiting to be swapped in and still in
    /// their queue. A device found twice keeps the newest, the one found last; so does what its
    /// slots knew about it.
    pub(crate) fn take_processors(&mut self, current: &mut Graph, taken: &mut Taken) {
        if let Some(held) = self.held.as_mut() {
            taken.take_graph(held);
        }
        taken.take_graph(current);
        for (key, processor) in self.parked.drain(..) {
            taken.processors.insert(key, processor);
        }
        if let Some(pending) = self.pending.as_mut() {
            taken.take_graph(&mut pending.graph);
        }
        if let Some(incoming) = self.incoming.as_mut() {
            while let Ok(mut swap) = incoming.pop() {
                taken.take_graph(&mut swap.graph);
            }
        }
        if let Some(queue) = self.plugins.as_mut() {
            while let Ok((key, processor)) = queue.pop() {
                taken.processors.insert(key, processor);
            }
        }
    }
}

fn park(parked: &mut Vec<(u64, Processor)>, retired: &mut Producer<Processor>, key: u64, processor: Processor) {
    if let Some(index) = parked.iter().position(|(k, _)| *k == key) {
        let (_, old) = parked.swap_remove(index);
        let _full = retired.push(old);
    }
    if parked.len() < parked.capacity() {
        parked.push((key, processor));
    } else {
        let _full = retired.push(processor);
    }
}

/// The sends of `chains` a knob's turn reaches as a message: (track, return).
fn live_sends(chains: &[crate::chain::Chain]) -> Vec<(TrackId, TrackId)> {
    chains.iter().flat_map(|c| c.live_sends().map(move |target| (c.track, target))).collect()
}

fn apply_param(chains: &mut [crate::chain::Chain], change: ParamChange) {
    if let Some(chain) = chains.iter_mut().find(|c| c.track == change.track) {
        if change.device == crate::MIXER_VOLUME {
            chain.set_live_volume(change.value);
        } else if change.device == crate::MIXER_PAN {
            chain.set_live_pan(change.value);
        } else if change.device == crate::MIXER_SEND {
            chain.set_live_send(TrackId(u32::try_from(change.param).unwrap_or(u32::MAX)), change.value);
        } else {
            chain.set_param(change.device, change.param, change.value);
        }
    }
}

impl Controller {
    /// Makes the renderer play `project` from now on, keeping what is playing (ADR 0005): builds
    /// it on this thread and swaps it in about [`SWAP_AHEAD_SECONDS`] from now. Fails if the
    /// project cannot be rendered or the renderer has not taken the previous updates yet.
    pub fn update(&mut self, project: &Project) -> Result<(), String> {
        self.update_with_scenes(project, None)
    }

    /// Like [`Controller::update`], for a project whose launcher differs from the one sent
    /// before by the inserted or removed scene `scenes` ([`SceneShift::between`]): what each
    /// column plays and has queued moves with its slot.
    pub fn update_with_scenes(&mut self, project: &Project, scenes: Option<SceneShift>) -> Result<(), String> {
        let updater = &mut self.updater;
        let collect_began = std::time::Instant::now();
        updater.collect();
        let collect_took = collect_began.elapsed();
        let mut plugins = Plugins { info: updater.plugin_info.clone(), processors: HashMap::new() };
        let mut graph = Graph::build(project, &updater.audio, &updater.assets, updater.sample_rate, &mut plugins, None)
            .map_err(|e| e.to_string())?;
        crate::build_report::add(&format!("old graphs dropped {:.1} ms", collect_took.as_secs_f64() * 1000.0));
        let sent = Arc::clone(&graph.tempo);
        updater.live_volumes = graph.chains.iter().filter(|c| c.built_volume.is_some()).map(|c| c.track).collect();
        updater.live_sends = live_sends(&graph.chains);
        updater.live_pans = graph.chains.iter().filter(|c| c.built_pan.is_some()).map(|c| c.track).collect();
        let monitor = &self.monitor;
        let (at, arrangement) = if monitor.running() {
            let ahead =
                if graph.ahead.is_empty() { 0 } else { (SWAP_AHEAD_SECONDS * f64::from(updater.sample_rate)) as u64 };
            let position = monitor.position.load(Ordering::Relaxed);
            let arrangement = monitor.arrangement.load(Ordering::Relaxed);
            // Arrangement frames are in the playing graph's tempo map; the swap keeps the beat, so
            // the new graph's voices go to the same beat in its own map. At constant tempos the
            // playing graph's frames per beat come from the monitor, exact even with an edit
            // still on its way; a changing map is the last one sent.
            let at = arrangement + ahead;
            let moved = if updater.tempo.is_constant() && graph.tempo.is_constant() {
                let playing = f64::from_bits(monitor.frames_per_beat.load(Ordering::Relaxed));
                crate::tempo::rescale_constant(at, playing, graph.tempo.start_per_beat())
            } else {
                crate::tempo::rescale(at, &updater.tempo, &graph.tempo)
            };
            (position + ahead, Some(moved))
        } else {
            (0, None)
        };
        if let Some(arrangement) = arrangement {
            graph.ahead.move_to(arrangement);
        }
        // The worker may take voices over from the playing graph: it goes to the stretch thread
        // only when the swap is sure to be queued (this is the queue's one producer), or those
        // voices would go with this graph.
        if updater.swaps.is_full() {
            return Err("the audio engine has not taken the previous edits yet".to_string());
        }
        if let Some(thread) = updater.stretch.as_mut() {
            let began = std::time::Instant::now();
            // The thread takes the unchanged clips' voices over and renders the rest (#743).
            // The launcher's stretch voices are built there too, not here (#793). First: should
            // the worker below not get in, this one costs nothing to lose.
            if let Some(preparer) = graph.launcher.take_preparer() {
                thread.add_preparer(preparer)?;
            }
            if let Some(worker) = graph.ahead.take_worker_unfilled() {
                thread.add(worker)?;
            }
            crate::build_report::add(&format!("rings filled {:.1} ms", began.elapsed().as_secs_f64() * 1000.0));
        }
        updater
            .swaps
            .push(Swap { graph, at, arrangement, param_seq: updater.param_seq, scenes })
            .map_err(|_| "the audio engine has not taken the previous edits yet".to_string())
            .map(|()| updater.tempo = sent)
    }

    /// Sets parameter `param` (its index in the device's info) of the device at index `device`
    /// among `track`'s channel's devices, from the start of the next audio block, without
    /// building a new graph. The project should hold the new value already, so the next graph
    /// built from it keeps it. Fails if the message queue is full.
    pub fn set_device_param(&mut self, track: TrackId, device: usize, param: usize, value: f64) -> Result<(), String> {
        let updater = &mut self.updater;
        // A fader move ([`crate::MIXER_VOLUME`]) only reaches a track whose chain can take it.
        if device == crate::MIXER_VOLUME && !updater.live_volumes.contains(&track) {
            return Err("the track's volume takes a new graph".to_string());
        }
        // A send's turn only reaches a send the playing graph has.
        let target = TrackId(u32::try_from(param).unwrap_or(u32::MAX));
        if device == crate::MIXER_SEND && !updater.live_sends.contains(&(track, target)) {
            return Err("the send takes a new graph".to_string());
        }
        // A pan on a lane, or a group's, takes a new graph.
        if device == crate::MIXER_PAN && (!updater.live_pans.contains(&track) || !value.is_finite()) {
            return Err("the track's pan takes a new graph".to_string());
        }
        let seq = updater.param_seq + 1;
        updater
            .params
            .push(ParamChange { seq, track, device, param, value })
            .map_err(|_| "the audio engine has not taken the previous parameter changes yet".to_string())?;
        updater.param_seq = seq;
        Ok(())
    }

    /// Tells the graphs built from now on what is known about the project's plug-in devices
    /// (ADR 0006): which have a slot, which are instruments, the delay each adds. Takes effect
    /// with the next [`Controller::update`].
    pub fn set_plugin_info(&mut self, info: HashMap<u64, PluginSlotInfo>) {
        self.updater.plugin_info = info;
    }

    /// What the graphs are built with about plug-in devices.
    pub fn plugin_info(&self) -> &HashMap<u64, PluginSlotInfo> {
        &self.updater.plugin_info
    }

    /// Sends the running plug-in `processor` for the plug-in device `key` to the audio thread:
    /// it plays from the next block in which a graph has its slot, replacing any processor
    /// there (a restarted plug-in replaces its crashed one). Fails if the queue is full.
    pub fn send_plugin(&mut self, key: u64, processor: Box<dyn PluginProcessor>) -> Result<(), String> {
        let updater = &mut self.updater;
        updater.collect();
        updater
            .plugins
            .push((key, processor))
            .map_err(|_| "the audio engine has not taken the previous plug-ins yet".to_string())
    }

    /// Drops processors the audio thread has given back (replaced, or of removed devices), and
    /// what the preview voice let go of.
    pub fn collect(&mut self) {
        self.updater.collect();
        self.preview.collect();
    }

    /// Builds the graphs from now on with `audio` (ADR 0011): the session's pool after files
    /// were added. Takes effect with the next [`Controller::update`].
    pub fn set_audio(&mut self, audio: Arc<AudioPool>) {
        self.updater.audio = audio;
    }

    /// The audio the graphs are built with.
    pub fn audio(&self) -> &Arc<AudioPool> {
        &self.updater.audio
    }

    /// Builds the graphs from now on with `assets` ([docs/spec/nam-amp-and-cab.md]): the
    /// session's parsed models and IRs after a device's file changed. Takes effect with the
    /// next [`Controller::update`].
    pub fn set_assets(&mut self, assets: Arc<crate::DeviceAssets>) {
        self.updater.assets = assets;
    }

    /// The files the graphs are built with.
    pub fn assets(&self) -> &Arc<crate::DeviceAssets> {
        &self.updater.assets
    }

    /// Previews `data` from `from` seconds at `gain` (linear), replacing any preview (ADR 0011).
    /// Fails if the queue is full.
    pub fn preview_sample(&mut self, data: Arc<crate::AudioData>, from: f64, gain: f32) -> Result<(), String> {
        let from = if from.is_finite() { from.max(0.0) } else { 0.0 };
        let gain = if gain.is_finite() { gain.clamp(0.0, 4.0) } else { 0.0 };
        self.preview.send(crate::preview::PreviewCommand::Sample { data, from, gain })
    }

    /// Previews notes ([`crate::preview_notes`] makes them) through their track's instrument,
    /// or through their fallback instrument when the track has none; the fallback plays at
    /// `gain` (linear). Replaces any preview. Fails if the queue is full.
    pub fn preview_notes(&mut self, notes: crate::PreviewNotes, gain: f32) -> Result<(), String> {
        let gain = if gain.is_finite() { gain.clamp(0.0, 4.0) } else { 0.0 };
        self.preview.send(crate::preview::PreviewCommand::Notes { notes: Box::new(notes), gain })
    }

    /// Starts the notes ([`crate::preview_notes`] makes them) and holds them until
    /// [`Controller::preview_note_off`]: the piano roll's key column (`preview.noteOn`).
    /// Replaces any preview. Fails if the queue is full.
    pub fn preview_note_on(&mut self, notes: crate::PreviewNotes, gain: f32) -> Result<(), String> {
        let gain = if gain.is_finite() { gain.clamp(0.0, 4.0) } else { 0.0 };
        self.preview.send(crate::preview::PreviewCommand::Hold { notes: Box::new(notes), gain })
    }

    /// Releases the held note of `key`, or every held note when `key` is `None`. Fails if the
    /// queue is full.
    pub fn preview_note_off(&mut self, key: Option<u8>) -> Result<(), String> {
        self.preview.send(crate::preview::PreviewCommand::Release { key })
    }

    /// A fallback instrument for [`crate::preview_notes`]: the FM synth with its first voice,
    /// prepared at the renderer's rate.
    pub fn preview_instrument(&self) -> Option<Box<dyn suhina_device_api::Device>> {
        let mut device = suhina_device_registry::create("fm-synth")?;
        device.prepare(f64::from(self.updater.sample_rate)).ok()?;
        device.reset();
        Some(device)
    }

    /// Changes the level of what is previewed now to `gain` (linear), gliding there without a
    /// click. Fails if the queue is full.
    pub fn preview_gain(&mut self, gain: f32) -> Result<(), String> {
        let gain = if gain.is_finite() { gain.clamp(0.0, 4.0) } else { 0.0 };
        self.preview.send(crate::preview::PreviewCommand::Gain(gain))
    }

    /// Stops the preview. Fails if the queue is full.
    pub fn preview_stop(&mut self) -> Result<(), String> {
        self.preview.send(crate::preview::PreviewCommand::Stop)
    }

    /// Whether a preview plays, and where.
    pub fn preview_monitor(&self) -> &crate::PreviewMonitor {
        &self.preview_monitor
    }

    /// The renderer's sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.updater.sample_rate
    }

    /// Runs the renderer's stretch worker on a thread of its own (ADR 0003), for this graph and
    /// every graph [`Controller::update`] sends later, and builds the launcher's stretch voices
    /// there. Call it before the renderer goes to the audio thread; the thread stops when the
    /// controller is dropped.
    pub fn run_stretch_worker(&mut self, renderer: &mut Renderer) -> std::io::Result<()> {
        let mut thread = StretchThread::spawn()?;
        if let Some(worker) = renderer.take_stretch_worker() {
            thread.add(worker).map_err(std::io::Error::other)?;
        }
        if let Some(preparer) = renderer.graph.launcher.take_preparer() {
            thread.add_preparer(preparer).map_err(std::io::Error::other)?;
        }
        self.updater.stretch = Some(thread);
        Ok(())
    }

    /// Frames of stretched clips that were not ready in time and played as silence.
    pub fn stretch_underruns(&self) -> u64 {
        self.monitor.stretch_underruns.load(Ordering::Relaxed)
    }

    /// Graph swaps while playing that moved the stretch voices, so far.
    pub fn stretch_moves(&self) -> u64 {
        self.monitor.stretch_moves.load(Ordering::Relaxed)
    }

    /// Plug-in blocks the playing graph's plug-ins did not answer in time, and the plug-in (its
    /// key) that missed most with its count.
    pub fn plugin_misses(&self) -> (u64, u64, u64) {
        let m = &self.monitor;
        (
            m.plugin_misses.load(Ordering::Relaxed),
            m.worst_plugin.load(Ordering::Relaxed),
            m.worst_misses.load(Ordering::Relaxed),
        )
    }
}
