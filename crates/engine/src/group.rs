// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device groups in a chain (`docs/spec/device-groups.md` › Engine).
//!
//! A group is one slot of a [`Chain`](crate::chain::Chain): every branch gets a copy of the
//! group's input, runs its devices over it in order (the track's notes reach every branch's
//! instrument at their frames), and the branches are scaled by their volume and pan and summed.
//! The output is `dry × (1 − mix) + sum × mix`, times the output gain; mix and gain are smoothed
//! over 20 ms, and so are the branches' gains. A muted branch, or one left out while another is soloed, still runs its devices
//! and adds nothing. Every buffer is made when the graph is built, so running a group never
//! allocates, locks or waits beyond what its plug-ins do.
//!
//! Latency: a branch's is the sum of its devices', the group's the largest of its branches' (at
//! most 2 s). Each branch is delayed by what it lacks of the group's, the dry signal by the
//! group's, so all of them line up, and the group reports its latency to the chain it is in.

use std::time::Instant;

use suhina_device_api::{NoteEvent, ProcessContext, Role};
use suhina_model::{DeviceKind, DeviceParam};

use crate::automation::DeviceLanes;
use crate::chain::{MAX_BLOCK, Slot};
use crate::plugin::{Delay, PluginDevice, PluginSlot, Plugins};
use crate::plugin_params::SlotParams;

/// The parameters of a device group, by their stable ids, in index order: `(id, min, max,
/// default)`. `mix` is a fraction, `gain` decibels (at or below [`SILENT_DB`] silent), the
/// macros fractions.
pub const GROUP_PARAMS: [(&str, f64, f64, f64); 10] = [
    ("mix", 0.0, 1.0, 1.0),
    ("gain", SILENT_DB, 12.0, 0.0),
    ("macro.1", 0.0, 1.0, 0.0),
    ("macro.2", 0.0, 1.0, 0.0),
    ("macro.3", 0.0, 1.0, 0.0),
    ("macro.4", 0.0, 1.0, 0.0),
    ("macro.5", 0.0, 1.0, 0.0),
    ("macro.6", 0.0, 1.0, 0.0),
    ("macro.7", 0.0, 1.0, 0.0),
    ("macro.8", 0.0, 1.0, 0.0),
];
/// A group gain at or below this many decibels is silence (the knob's −inf).
pub const SILENT_DB: f64 = -96.0;
const MIX: usize = 0;
const GAIN: usize = 1;
/// Events one branch holds for a block, as a chain does.
const MAX_EVENTS: usize = 512;

/// The index of a device group's parameter `id` (`mix`, `gain`, `macro.1` … `macro.8`).
pub fn group_param_index(id: &str) -> Option<usize> {
    GROUP_PARAMS.iter().position(|(name, ..)| *name == id)
}

/// The values of a device group's parameters from its stored `params`, each limited to its
/// range, in [`GROUP_PARAMS`] order.
pub fn group_param_values(params: &[DeviceParam]) -> Vec<f64> {
    GROUP_PARAMS
        .iter()
        .map(|&(id, min, max, default)| {
            params
                .iter()
                .find(|p| p.id == id)
                .map_or(default, |p| if p.value.is_finite() { p.value.clamp(min, max) } else { default })
        })
        .collect()
}

/// Linear gain of `db` decibels, silence at or below [`SILENT_DB`].
fn db_to_gain(db: f64) -> f32 {
    if db <= SILENT_DB { 0.0 } else { 10f64.powf(db.min(12.0) / 20.0) as f32 }
}

/// A value that moves towards its target over about 20 ms, one frame at a time.
#[derive(Clone, Copy)]
struct Smoothed {
    value: f32,
    target: f32,
    coefficient: f32,
}

impl Smoothed {
    fn new(value: f32, sample_rate: u32) -> Self {
        // A one-pole filter whose time constant puts it within 1 % of the target after 20 ms.
        let tau = 0.02 / 4.6 * f64::from(sample_rate.max(1));
        Self { value, target: value, coefficient: (1.0 - (-1.0 / tau).exp()) as f32 }
    }

    fn next(&mut self) -> f32 {
        if self.value != self.target {
            self.value += (self.target - self.value) * self.coefficient;
            if (self.target - self.value).abs() < 1e-6 {
                self.value = self.target;
            }
        }
        self.value
    }
}

/// One branch of a running group.
pub(crate) struct BranchRun {
    pub(crate) slots: Vec<Slot>,
    /// Each slot's parameter values, as the chain keeps them, so a graph swap can set the new
    /// values on running devices.
    values: Vec<Vec<f64>>,
    ids: Vec<String>,
    /// Each slot's manual sample slices, as the chain keeps them, so a graph swap can hand the
    /// new ones to a running Sampler.
    sample_slices: Vec<Option<std::sync::Arc<[f64]>>>,
    /// The lanes of its devices' automated parameters, by slot in the branch.
    lanes: DeviceLanes,
    buffer: Vec<f32>,
    /// Its gains, smoothed: its volume and pan while it is heard, 0 while it is muted or
    /// another branch is soloed.
    left: Smoothed,
    right: Smoothed,
    /// The delay its devices add together, in frames.
    pub(crate) latency: u32,
    /// Delays its output by what it lacks of the group's latency, so every branch lines up.
    delay: Delay,
}

/// A device group in a chain.
pub(crate) struct GroupSlot {
    pub(crate) branches: Vec<BranchRun>,
    mix: Smoothed,
    gain: Smoothed,
    /// Its parameter values, in [`GROUP_PARAMS`] order.
    values: Vec<f64>,
    /// Whether its branches start with an instrument: it plays the notes and has no dry signal.
    pub(crate) instrument: bool,
    /// The dry signal of the current span, delayed by the group's latency.
    dry: Vec<f32>,
    dry_delay: Delay,
    events: Vec<(usize, NoteEvent)>,
    span_events: Vec<(usize, NoteEvent)>,
    midi_scratch: Vec<(usize, NoteEvent)>,
}

/// What builds the devices inside a group: the sample rate, the decoded audio a Sampler plays,
/// the files of file-backed devices, the plug-ins (their slots and processors, by the keys of the
/// channel's plug-in devices).
pub(crate) struct Builder<'a> {
    pub(crate) sample_rate: u32,
    pub(crate) audio: &'a crate::AudioPool,
    pub(crate) assets: &'a crate::DeviceAssets,
    pub(crate) plugins: &'a mut Plugins,
    pub(crate) keys: &'a [PluginDevice],
    pub(crate) channel: suhina_model::ChannelId,
    pub(crate) frames_per_beat: f64,
}

/// A slot built for a stored device: the slot, its identity for graph swaps, its parameter
/// values, its latency and its manual sample slices.
pub(crate) struct Built {
    pub(crate) slot: Slot,
    pub(crate) id: String,
    pub(crate) values: Vec<f64>,
    pub(crate) latency: u32,
    pub(crate) slices: Option<std::sync::Arc<[f64]>>,
}

impl Builder<'_> {
    /// The slot of the device at `path` on the channel, inside a group, or `None` for one that
    /// does not run there: switched off, unknown, not compiling, or a plug-in that is not
    /// installed. A switched-off plug-in keeps its slot, and its process, and passes the sound.
    /// `lanes` is the branch's, and `slot` the index the device takes there: the lanes of its
    /// automated parameters are added under it. Allocates.
    pub(crate) fn device(
        &mut self,
        stored: &suhina_model::Device,
        depth: usize,
        path: &[usize],
        (lanes, slot): (&mut DeviceLanes, usize),
    ) -> Option<Built> {
        if matches!(stored.kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. }) {
            return self.plugin(stored, path);
        }
        if !stored.enabled {
            return None;
        }
        match &stored.kind {
            DeviceKind::Builtin(id) => self.builtin(id, stored, lanes, slot),
            DeviceKind::Sd => self.sd(stored, lanes, slot),
            DeviceKind::Group(group) => self.group(stored, group, depth, path),
            _ => None,
        }
    }

    /// A plug-in's slot, by its key: it takes its processor from the plug-ins, or waits for one.
    fn plugin(&mut self, stored: &suhina_model::Device, path: &[usize]) -> Option<Built> {
        let key = self.keys.iter().find(|d| d.channel == self.channel && d.path == path)?.key;
        let info = *self.plugins.info.get(&key)?;
        let slot = PluginSlot {
            key,
            processor: self.plugins.processors.remove(&key),
            instrument: info.instrument,
            latency: info.latency,
            enabled: stored.enabled,
            missed: 0,
            busy: 0,
            longest_us: 0,
            sent_at: None,
            params: SlotParams::new(stored, self.frames_per_beat),
        };
        let latency = if stored.enabled { info.latency } else { 0 };
        Some(Built {
            slot: Slot::Plugin(slot),
            id: format!("plugin:{key:016x}"),
            values: Vec::new(),
            latency,
            slices: None,
        })
    }

    fn builtin(&self, id: &str, stored: &suhina_model::Device, lanes: &mut DeviceLanes, slot: usize) -> Option<Built> {
        let asset = crate::assets::stored_path(id, stored.state.as_deref()).and_then(|path| self.assets.get(path));
        let mut device = suhina_device_registry::create_with(id, asset)?;
        // What a chain gives its Sampler: the decoded sample and the manual slices.
        if let Some(source) = stored.sample_file().and_then(|file| self.audio.files.get(file)) {
            device.load_shared_sample(source.sample_rate, source.channels, source.clone());
        }
        let slices: Option<std::sync::Arc<[f64]>> = stored.sample_slices().map(std::sync::Arc::from);
        device.load_sample_slices(slices.clone());
        device.prepare(f64::from(self.sample_rate)).ok()?;
        let info = device.info();
        let mut values: Vec<f64> = info.params.iter().map(|p| p.default).collect();
        for p in &stored.params {
            if let Some(slot) = info.param_index(&p.id).and_then(|i| values.get_mut(i)) {
                *slot = p.value;
            }
        }
        lanes.add(slot, device.as_ref(), &stored.automation, self.frames_per_beat);
        for (index, &value) in values.iter().enumerate() {
            // A parameter that follows a lane starts where the lane starts.
            device.set_param(index, lanes.initial(slot, index).unwrap_or(value));
        }
        device.reset();
        let latency = device.latency();
        // A built-in MIDI effect runs on the events, as in a chain (#678).
        let kind = if info.role == Role::MidiEffect { Slot::MidiDevice(device) } else { Slot::Device(device) };
        Some(Built { slot: kind, id: crate::chain::device_identity(id, stored, self.audio), values, latency, slices })
    }

    fn sd(&self, stored: &suhina_model::Device, lanes: &mut DeviceLanes, slot: usize) -> Option<Built> {
        let bytes = stored.state.as_deref()?;
        let text = std::str::from_utf8(bytes).ok()?;
        let graph = suhina_device_sdk::DeviceGraph::from_sd(text).ok()?;
        let mut samples = suhina_device_sdk::sample_resource::PreparedSamples::default();
        if let Some(source) = stored.sample_file().and_then(|file| self.audio.files.get(file)) {
            let asset = suhina_device_sdk::sample_resource::SampleAsset::new(
                source.clone(),
                source.sample_rate,
                source.channels,
            )
            .ok()?;
            samples.insert("sample", std::sync::Arc::new(asset)).ok()?;
        }
        let rate = f64::from(self.sample_rate);
        let mut built = match graph.role {
            suhina_device_sdk::DeviceRole::Effect => {
                Slot::Sd(suhina_device_registry::compile_graph_with_samples(&graph, rate, &samples).ok()?)
            }
            suhina_device_sdk::DeviceRole::MidiEffect => Slot::MidiSd(graph.compile_midi().ok()?),
            suhina_device_sdk::DeviceRole::Instrument => {
                Slot::InstrumentSd(suhina_device_registry::compile_graph_with_samples(&graph, rate, &samples).ok()?)
            }
        };
        lanes.add_sd(slot, &graph.parameters, &stored.automation, self.frames_per_beat);
        let mut values = Vec::with_capacity(graph.parameters.len());
        for (index, parameter) in graph.parameters.iter().enumerate() {
            let value = stored
                .params
                .iter()
                .find(|value| value.id == parameter.id)
                .map_or(parameter.default, |value| parameter.clamp(value.value));
            set_sd(&mut built, index, lanes.initial(slot, index).unwrap_or(value));
            values.push(value);
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        stored.sample_file().hash(&mut hasher);
        stored.sample_file().and_then(|file| self.audio.files.get(file)).map(std::sync::Arc::as_ptr).hash(&mut hasher);
        Some(Built {
            slot: built,
            id: format!("sd:{}:{:016x}", graph.id, hasher.finish()),
            values,
            latency: 0,
            slices: None,
        })
    }

    /// A group of the stored `group` at `path` on the channel, with its branches built; `depth`
    /// is how many groups it is in. A group never runs deeper than the model allows, so a
    /// malformed one cannot recurse without end.
    pub(crate) fn group(
        &mut self,
        stored: &suhina_model::Device,
        group: &suhina_model::DeviceGroup,
        depth: usize,
        path: &[usize],
    ) -> Option<Built> {
        if depth >= suhina_model::MAX_DEVICE_GROUP_DEPTH {
            return None;
        }
        let any_solo = group.branches.iter().any(|b| b.solo);
        let mut branches = Vec::with_capacity(group.branches.len());
        let mut ids = Vec::with_capacity(group.branches.len());
        for (b, branch) in group.branches.iter().enumerate().take(suhina_model::MAX_BRANCHES) {
            let mut run = BranchRun {
                slots: Vec::new(),
                values: Vec::new(),
                ids: Vec::new(),
                sample_slices: Vec::new(),
                lanes: DeviceLanes::none(),
                buffer: vec![0.0; MAX_BLOCK * 2],
                left: Smoothed::new(0.0, self.sample_rate),
                right: Smoothed::new(0.0, self.sample_rate),
                latency: 0,
                delay: Delay::new(0),
            };
            for (i, device) in branch.devices.iter().enumerate() {
                let mut inner = path.to_vec();
                inner.extend([b, i]);
                let slot = run.slots.len();
                if let Some(built) = self.device(device, depth + 1, &inner, (&mut run.lanes, slot)) {
                    run.latency = run.latency.saturating_add(built.latency);
                    run.slots.push(built.slot);
                    run.ids.push(built.id);
                    run.values.push(built.values);
                    run.sample_slices.push(built.slices);
                }
            }
            let (left, right) = heard_gains(branch, any_solo);
            (run.left.value, run.left.target, run.right.value, run.right.target) = (left, left, right, right);
            ids.push(run.ids.join(","));
            branches.push(run);
        }
        let values = group_param_values(&stored.params);
        let mix = values.get(MIX).copied().unwrap_or(1.0);
        let gain = values.get(GAIN).copied().unwrap_or(0.0);
        let instrument = branches.iter().any(|b| b.slots.first().is_some_and(is_instrument));
        let cap = self.sample_rate.saturating_mul(crate::MAX_LATENCY_SECONDS);
        let (latency, delays) = compensation(branches.iter().map(|b| b.latency), cap);
        for (branch, delay) in branches.iter_mut().zip(delays) {
            branch.delay = Delay::new(delay as usize);
        }
        let slot = GroupSlot {
            branches,
            // An instrument group has no dry signal.
            mix: Smoothed::new(if instrument { 1.0 } else { mix as f32 }, self.sample_rate),
            gain: Smoothed::new(db_to_gain(gain), self.sample_rate),
            values: values.clone(),
            instrument,
            dry: vec![0.0; MAX_BLOCK * 2],
            dry_delay: Delay::new(if instrument { 0 } else { latency as usize }),
            events: Vec::with_capacity(MAX_EVENTS),
            span_events: Vec::with_capacity(MAX_EVENTS),
            midi_scratch: Vec::with_capacity(MAX_EVENTS),
        };
        Some(Built {
            slot: Slot::Group(Box::new(slot)),
            id: format!("group[{}]", ids.join("|")),
            values,
            latency,
            slices: None,
        })
    }
}

/// The latency of a group whose branches have `latencies`, at most `cap` frames, and the delay
/// each branch needs to line up with it: the slowest branch gets none. A branch slower than the
/// cap is cut to it and is not fully compensated.
pub(crate) fn compensation(latencies: impl Iterator<Item = u32>, cap: u32) -> (u32, Vec<u32>) {
    let latencies: Vec<u32> = latencies.map(|l| l.min(cap)).collect();
    let most = latencies.iter().copied().max().unwrap_or(0);
    (most, latencies.iter().map(|&l| most - l).collect())
}

/// A branch's gains while `any_solo` says whether a branch of its group is soloed: 0 while it
/// is muted or left out by a solo, else [`branch_gains`].
fn heard_gains(branch: &suhina_model::Branch, any_solo: bool) -> (f32, f32) {
    if branch.mute || (any_solo && !branch.solo) { (0.0, 0.0) } else { branch_gains(branch.volume, branch.pan) }
}

/// A branch's gains: its volume, and its pan by the equal-power law a track's channel uses,
/// centred at unity.
fn branch_gains(volume: f64, pan: f64) -> (f32, f32) {
    let volume = if volume.is_finite() { volume.max(0.0) } else { 0.0 };
    let (left, right) = suhina_dsp::pan::gains(pan);
    let scale = volume * std::f64::consts::SQRT_2;
    ((left * scale) as f32, (right * scale) as f32)
}

pub(crate) fn is_instrument(slot: &Slot) -> bool {
    match slot {
        Slot::Device(device) => device.info().role == Role::Instrument,
        Slot::Plugin(slot) => slot.instrument,
        Slot::InstrumentSd(_) => true,
        Slot::Group(group) => group.instrument,
        Slot::MidiDevice(_) | Slot::Sd(_) | Slot::MidiSd(_) => false,
    }
}

fn set_sd(slot: &mut Slot, index: usize, value: f64) {
    match slot {
        Slot::Sd(effect) | Slot::InstrumentSd(effect) => effect.set_parameter(index, value),
        Slot::MidiSd(effect) => effect.set_parameter(index, value),
        Slot::Device(_) | Slot::MidiDevice(_) | Slot::Plugin(_) | Slot::Group(_) => {}
    }
}

impl GroupSlot {
    /// Sets its parameter `param` ([`GROUP_PARAMS`] order). Never allocates.
    pub(crate) fn set_param(&mut self, param: usize, value: f64) {
        let Some(&(_, min, max, _)) = GROUP_PARAMS.get(param) else { return };
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(min, max);
        if let Some(slot) = self.values.get_mut(param) {
            *slot = value;
        }
        match param {
            MIX if !self.instrument => self.mix.target = value as f32,
            GAIN => self.gain.target = db_to_gain(value),
            _ => {}
        }
    }

    /// Plans the changes of its devices' lanes, in every branch and group inside it, for a block
    /// of `frames` frames from arrangement frame `time` (`None` while stopped). Never allocates.
    pub(crate) fn plan(&mut self, time: Option<u64>, frames: usize) {
        for branch in &mut self.branches {
            branch.lanes.plan(time, frames);
            for slot in &mut branch.slots {
                if let Slot::Group(group) = slot {
                    group.plan(time, frames);
                }
            }
        }
    }

    /// Its parameter values, in [`GROUP_PARAMS`] order.
    pub(crate) fn values(&self) -> &[f64] {
        &self.values
    }

    /// Takes the settings of `fresh`, a group built for the same devices after an edit: branch
    /// gains, mute and solo, mix and gain (smoothed from where they are), and its devices'
    /// parameter values. Its own running devices stay. Never allocates.
    pub(crate) fn adopt(&mut self, fresh: &mut GroupSlot) {
        std::mem::swap(&mut self.dry_delay, &mut fresh.dry_delay);
        self.dry_delay.adopt(&mut fresh.dry_delay);
        self.mix.target = fresh.mix.target;
        self.gain.target = fresh.gain.target;
        for (value, &new) in self.values.iter_mut().zip(&fresh.values) {
            *value = new;
        }
        for (running, new) in self.branches.iter_mut().zip(fresh.branches.iter_mut()) {
            running.left.target = new.left.target;
            running.right.target = new.right.target;
            // A plug-in's latency can change without its identity: take the new delay line,
            // with what the old one holds when it is as long.
            std::mem::swap(&mut running.delay, &mut new.delay);
            running.delay.adopt(&mut new.delay);
            let lanes = &new.lanes;
            for (position, ((slot, values), new_slot)) in
                running.slots.iter_mut().zip(&new.values).zip(new.slots.iter_mut()).enumerate()
            {
                match (slot, new_slot) {
                    (Slot::Device(device) | Slot::MidiDevice(device), _) => {
                        if let Some(points) = new.sample_slices.get_mut(position) {
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
                            set_sd(slot, index, value);
                        }
                    }
                    (Slot::Group(group), Slot::Group(new_group)) => group.adopt(new_group),
                    (Slot::Plugin(running), Slot::Plugin(fresh)) => running.adopt(fresh),
                    _ => {}
                }
            }
            std::mem::swap(&mut running.lanes, &mut new.lanes);
            for (values, new_values) in running.values.iter_mut().zip(&new.values) {
                for (value, &new) in values.iter_mut().zip(new_values) {
                    *value = new;
                }
            }
        }
    }

    /// Runs the group over `buffer`'s frames `from..to` (interleaved stereo, frames counted from
    /// the block's start), with `pending`, the notes at their frames in the block. Never
    /// allocates.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        buffer: &mut [f32],
        from: usize,
        to: usize,
        pending: &[(usize, NoteEvent)],
        context: &ProcessContext,
        sample_rate: f64,
        deadline: Option<Instant>,
    ) {
        if to <= from {
            return;
        }
        let Self { branches, events, span_events, midi_scratch, mix, gain, dry, dry_delay, .. } = self;
        let Some(input) = buffer.get(from * 2..to * 2) else { return };
        let Some(dry) = dry.get_mut(from * 2..to * 2) else { return };
        dry.copy_from_slice(input);
        dry_delay.run(dry);
        for branch in branches.iter_mut() {
            let Some(span) = branch.buffer.get_mut(from * 2..to * 2) else { continue };
            span.copy_from_slice(input);
            events.clear();
            events.extend_from_slice(pending.get(..pending.len().min(MAX_EVENTS)).unwrap_or_default());
            let run = Span { from, to, context, sample_rate, deadline };
            run_slots(&mut branch.slots, &mut branch.buffer, events, span_events, midi_scratch, &branch.lanes, run);
            if let Some(span) = branch.buffer.get_mut(from * 2..to * 2) {
                branch.delay.run(span);
            }
        }
        for (frame, at) in (from..to).enumerate() {
            let (mut left, mut right) = (0.0f32, 0.0f32);
            for branch in branches.iter_mut() {
                let (gain_left, gain_right) = (branch.left.next(), branch.right.next());
                if let (Some(&l), Some(&r)) = (branch.buffer.get(at * 2), branch.buffer.get(at * 2 + 1)) {
                    left += l * gain_left;
                    right += r * gain_right;
                }
            }
            let mix = mix.next();
            let gain = gain.next();
            let (Some([out_left, out_right]), Some(&[dry_left, dry_right])) = (
                buffer.get_mut(at * 2..at * 2 + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok()),
                dry.get(frame * 2..frame * 2 + 2).and_then(|s| <&[f32; 2]>::try_from(s).ok()),
            ) else {
                continue;
            };
            *out_left = (dry_left * (1.0 - mix) + left * mix) * gain;
            *out_right = (dry_right * (1.0 - mix) + right * mix) * gain;
        }
    }
}

/// The plug-in slots among a chain's slots, inside device groups too, depth-first, without
/// allocating: a fixed stack of the lists being walked, deep enough for the deepest group.
pub(crate) struct PluginSlots<'a> {
    stack: [Option<Frame<'a>>; STACK],
    len: usize,
}

/// One list being walked: slots, or a group's branches.
enum Frame<'a> {
    Slots(std::slice::IterMut<'a, Slot>),
    Branches(std::slice::IterMut<'a, BranchRun>),
}

/// Lists walked at once at most: the chain's slots, then branches and slots for each group.
const STACK: usize = 2 + 2 * suhina_model::MAX_DEVICE_GROUP_DEPTH;

impl<'a> PluginSlots<'a> {
    pub(crate) fn new(slots: &'a mut [Slot]) -> Self {
        let mut stack: [Option<Frame<'a>>; STACK] = std::array::from_fn(|_| None);
        stack[0] = Some(Frame::Slots(slots.iter_mut()));
        Self { stack, len: 1 }
    }

    fn push(&mut self, frame: Frame<'a>) {
        // A tree deeper than the model allows is not walked further.
        if let Some(place) = self.stack.get_mut(self.len) {
            *place = Some(frame);
            self.len += 1;
        }
    }
}

impl<'a> Iterator for PluginSlots<'a> {
    type Item = &'a mut PluginSlot;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let top = self.len.checked_sub(1)?;
            let next = match self.stack.get_mut(top)?.as_mut()? {
                Frame::Slots(slots) => slots.next().map(Ok),
                Frame::Branches(branches) => branches.next().map(Err),
            };
            match next {
                Some(Ok(Slot::Plugin(plugin))) => return Some(plugin),
                Some(Ok(Slot::Group(group))) => self.push(Frame::Branches(group.branches.iter_mut())),
                Some(Ok(_)) => {}
                Some(Err(branch)) => self.push(Frame::Slots(branch.slots.iter_mut())),
                None => {
                    if let Some(frame) = self.stack.get_mut(top) {
                        *frame = None;
                    }
                    self.len = top;
                }
            }
        }
    }
}

/// The frames a branch runs and what it runs them with.
#[derive(Clone, Copy)]
pub(crate) struct Span<'a> {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) context: &'a ProcessContext,
    /// The song's sample rate, for each span's song position.
    pub(crate) sample_rate: f64,
    pub(crate) deadline: Option<Instant>,
}

/// Runs `slots` one after another over `buffer`'s frames `from..to`, each split at the events
/// in `pending` (frames counted from the block's start). A plug-in runs its round trip and waits
/// for it within the deadline. Never allocates.
pub(crate) fn run_slots(
    slots: &mut [Slot],
    buffer: &mut [f32],
    pending: &mut Vec<(usize, NoteEvent)>,
    span_events: &mut Vec<(usize, NoteEvent)>,
    midi_scratch: &mut Vec<(usize, NoteEvent)>,
    lanes: &DeviceLanes,
    run: Span<'_>,
) {
    let Span { from, to, context, sample_rate, deadline } = run;
    for (index, slot) in slots.iter_mut().enumerate() {
        match slot {
            Slot::Device(device) => {
                crate::chain::run_device(device, index, buffer, from, to, pending, lanes, context, sample_rate);
            }
            Slot::Sd(effect) => {
                crate::chain::run_sd_effect(effect, index, buffer, lanes, crate::chain::SdRun { from, to, context });
            }
            Slot::InstrumentSd(instrument) => {
                span_events.clear();
                let last = (to - from).saturating_sub(1);
                span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                crate::chain::run_sd_instrument(
                    instrument,
                    index,
                    buffer,
                    span_events,
                    midi_scratch,
                    lanes,
                    crate::chain::SdRun { from, to, context },
                );
            }
            // A built-in MIDI effect (#678) changes the branch's events as in a chain.
            Slot::MidiDevice(device) => {
                crate::chain::run_midi_device(
                    device,
                    index,
                    pending,
                    span_events,
                    midi_scratch,
                    lanes,
                    crate::chain::SdRun { from, to, context },
                );
                std::mem::swap(pending, midi_scratch);
            }
            Slot::MidiSd(effect) => {
                crate::chain::run_sd_midi(
                    effect,
                    index,
                    pending,
                    midi_scratch,
                    lanes,
                    crate::chain::SdRun { from, to, context },
                );
                std::mem::swap(pending, midi_scratch);
            }
            Slot::Plugin(plugin) => {
                let last = (to - from).saturating_sub(1);
                span_events.clear();
                span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                plugin.automate(from, to);
                if let Some(span) = buffer.get_mut(from * 2..to * 2) {
                    plugin.run(span, span_events, deadline);
                }
            }
            Slot::Group(group) => group.run(buffer, from, to, pending, context, sample_rate, deadline),
        }
    }
}
