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
use suhina_model::DeviceParam;

use crate::automation::DeviceLanes;
use crate::chain::{DeviceSlot, MAX_BLOCK};
use crate::plugin::{Delay, PluginSlot};
use crate::slot_factory::{Built, SlotFactory};

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
    pub(crate) slots: Vec<DeviceSlot>,
    /// Each slot's device in the stored branch: one that cannot run has no slot.
    positions: Vec<usize>,
    /// Each slot's parameter values, as the chain keeps them, so a graph swap can set the new
    /// values on running devices.
    values: Vec<Vec<f64>>,
    ids: Vec<String>,
    /// Each slot's manual sample slices, as the chain keeps them, so a graph swap can hand the
    /// new ones to a running Sampler.
    sample_slices: Vec<Option<std::sync::Arc<[f64]>>>,
    sample_edits: Vec<Option<std::sync::Arc<suhina_device_api::sample_edits::SampleEdits>>>,
    /// The lanes of its devices' automated parameters, by slot in the branch.
    lanes: DeviceLanes,
    buffer: Vec<f32>,
    /// Its gains, smoothed: its volume and pan while it is heard, 0 while it is muted or
    /// another branch is soloed.
    left: Smoothed,
    right: Smoothed,
    /// Its volume, pan, mute and solo, which its gains follow: a message moves them while the
    /// group plays (#763 › 4).
    mix: BranchMix,
    /// The delay its devices add together, in frames.
    pub(crate) latency: u32,
    /// Delays its output by what it lacks of the group's latency, so every branch lines up.
    delay: Delay,
}

/// A device group in a chain.
pub(crate) struct GroupSlot {
    /// Its number among the channel's groups, in depth-first order ([`crate::group_number`]):
    /// what a branch message names it by.
    pub(crate) number: usize,
    pub(crate) branches: Vec<BranchRun>,
    mix: Smoothed,
    gain: Smoothed,
    /// Its parameter values, in [`GROUP_PARAMS`] order.
    values: Vec<f64>,
    /// Whether a branch holds an instrument: it plays the notes and has no dry signal.
    pub(crate) instrument: bool,
    /// The dry signal of the current span, delayed by the group's latency.
    dry: Vec<f32>,
    dry_delay: Delay,
    events: Vec<(usize, NoteEvent)>,
    span_events: Vec<(usize, NoteEvent)>,
    midi_scratch: Vec<(usize, NoteEvent)>,
}

impl SlotFactory<'_> {
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
                positions: Vec::new(),
                values: Vec::new(),
                ids: Vec::new(),
                sample_slices: Vec::new(),
                sample_edits: Vec::new(),
                lanes: DeviceLanes::none(),
                buffer: vec![0.0; MAX_BLOCK * 2],
                left: Smoothed::new(0.0, self.sample_rate),
                right: Smoothed::new(0.0, self.sample_rate),
                mix: BranchMix { volume: branch.volume, pan: branch.pan, mute: branch.mute, solo: branch.solo },
                latency: 0,
                delay: Delay::new(0),
            };
            // What the branch runs, each device with its id: a group whose devices traded
            // places, even two of the same kind, starts again instead of each running device
            // taking the other's settings.
            let mut identity = Vec::new();
            for (i, device) in branch.devices.iter().enumerate() {
                let mut inner = path.to_vec();
                inner.extend([b, i]);
                let slot = run.slots.len();
                if let Some(built) = self.device(device, depth + 1, &inner, (&mut run.lanes, slot)) {
                    run.latency = run.latency.saturating_add(built.latency);
                    run.slots.push(built.slot);
                    run.positions.push(i);
                    identity.push(format!("{}#{}", built.id, device.id.0));
                    run.ids.push(built.id);
                    run.values.push(built.values);
                    run.sample_slices.push(built.slices);
                    run.sample_edits.push(built.edits);
                }
            }
            let (left, right) = run.mix.heard_gains(any_solo);
            (run.left.value, run.left.target, run.right.value, run.right.target) = (left, left, right, right);
            ids.push(identity.join(","));
            branches.push(run);
        }
        let values = group_param_values(&stored.params);
        let mix = values.get(MIX).copied().unwrap_or(1.0);
        let gain = values.get(GAIN).copied().unwrap_or(0.0);
        // A MIDI effect may lead the instrument in its branch.
        let instrument = branches.iter().any(|b| b.slots.iter().any(is_instrument));
        let cap = self.sample_rate.saturating_mul(crate::MAX_LATENCY_SECONDS);
        let (latency, delays) = compensation(branches.iter().map(|b| b.latency), cap);
        for (branch, delay) in branches.iter_mut().zip(delays) {
            branch.delay = Delay::new(delay as usize);
        }
        let slot = GroupSlot {
            number: usize::MAX,
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
            slot: DeviceSlot::Group(Box::new(slot)),
            id: format!("group[{}]", ids.join("|")),
            values,
            latency,
            slices: None,
            edits: None,
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

/// Hands `visit` the live values of each device among `slots` and inside their groups, with the
/// device's path (`crate::meters::readout_step`) from `parent`. Never allocates.
pub(crate) fn visit_readouts(
    slots: &[DeviceSlot],
    positions: &[usize],
    parent: u64,
    visit: &mut dyn FnMut(u64, [f32; suhina_device_api::MAX_READOUTS]),
) {
    for (slot, &position) in slots.iter().zip(positions) {
        let path = crate::meters::readout_step(parent, position);
        match slot {
            DeviceSlot::Device(device) => {
                if let Some(values) = device.readouts() {
                    visit(path, values);
                }
            }
            DeviceSlot::Group(group) => {
                for (index, branch) in group.branches.iter().enumerate() {
                    let branch_path = crate::meters::readout_step(path, index);
                    visit_readouts(&branch.slots, &branch.positions, branch_path, visit);
                }
            }
            _ => {}
        }
    }
}

/// What a branch's gains follow.
#[derive(Debug, Clone, Copy)]
struct BranchMix {
    volume: f64,
    pan: f64,
    mute: bool,
    solo: bool,
}

impl BranchMix {
    /// Its gains while `any_solo` says whether a branch of its group is soloed: 0 while it is
    /// muted or left out by a solo, else [`branch_gains`].
    fn heard_gains(&self, any_solo: bool) -> (f32, f32) {
        if self.mute || (any_solo && !self.solo) { (0.0, 0.0) } else { branch_gains(self.volume, self.pan) }
    }
}

/// A branch's gains: its volume, and its pan by the equal-power law a track's channel uses,
/// centred at unity.
fn branch_gains(volume: f64, pan: f64) -> (f32, f32) {
    let volume = if volume.is_finite() { volume.max(0.0) } else { 0.0 };
    let (left, right) = suhina_dsp::pan::gains(pan);
    let scale = volume * std::f64::consts::SQRT_2;
    ((left * scale) as f32, (right * scale) as f32)
}

pub(crate) fn is_instrument(slot: &DeviceSlot) -> bool {
    match slot {
        DeviceSlot::Device(device) => device.info().role == Role::Instrument,
        DeviceSlot::Plugin(slot) => slot.instrument,
        DeviceSlot::Group(group) => group.instrument,
        DeviceSlot::MidiDevice(_) => false,
    }
}

/// Numbers the groups among `slots` and inside them in depth-first order of the stored
/// `devices` they were built from (`positions` maps a slot to its device), as
/// [`crate::group_number`] counts: a group that does not run still takes its number, and so do
/// those inside it. Never allocates.
pub(crate) fn number_groups(
    slots: &mut [DeviceSlot],
    positions: &[usize],
    devices: &[suhina_model::Device],
    counter: &mut usize,
) {
    for (index, device) in devices.iter().enumerate() {
        let Some(group) = device.as_group() else { continue };
        let number = *counter;
        *counter += 1;
        let slot = positions.iter().position(|&p| p == index).and_then(|slot| slots.get_mut(slot));
        match slot {
            Some(DeviceSlot::Group(running)) => {
                running.number = number;
                for (b, branch) in group.branches.iter().enumerate() {
                    match running.branches.get_mut(b) {
                        Some(run) => number_groups(&mut run.slots, &run.positions, &branch.devices, counter),
                        None => *counter += crate::count_groups(&branch.devices),
                    }
                }
            }
            _ => *counter += group.branches.iter().map(|b| crate::count_groups(&b.devices)).sum::<usize>(),
        }
    }
}

/// The running group numbered `number` among `slots` and inside them. Never allocates.
pub(crate) fn find_group(slots: &mut [DeviceSlot], number: usize) -> Option<&mut GroupSlot> {
    for slot in slots {
        if let DeviceSlot::Group(group) = slot {
            if group.number == number {
                return Some(group);
            }
            for branch in &mut group.branches {
                if let Some(found) = find_group(&mut branch.slots, number) {
                    return Some(found);
                }
            }
        }
    }
    None
}

impl GroupSlot {
    /// Moves branch `branch`'s volume (linear, at most +6 dB), pan (-1 to 1), mute or solo (on
    /// at 0.5 and above): its gains, and with a solo every branch's, go to their new targets and
    /// get there smoothed. Ignored for a branch it does not have or a value that is not finite.
    /// Never allocates.
    pub(crate) fn set_branch(&mut self, branch: usize, what: crate::BranchValue, value: f64) {
        if !value.is_finite() {
            return;
        }
        let Some(run) = self.branches.get_mut(branch) else { return };
        match what {
            crate::BranchValue::Volume => run.mix.volume = value.clamp(0.0, suhina_model::automation::MAX_VOLUME),
            crate::BranchValue::Pan => run.mix.pan = value.clamp(-1.0, 1.0),
            crate::BranchValue::Mute => run.mix.mute = value >= 0.5,
            crate::BranchValue::Solo => run.mix.solo = value >= 0.5,
        }
        let any_solo = self.branches.iter().any(|b| b.mix.solo);
        for run in &mut self.branches {
            let (left, right) = run.mix.heard_gains(any_solo);
            run.left.target = left;
            run.right.target = right;
        }
    }

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
                if let DeviceSlot::Group(group) = slot {
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
    /// parameter values, and its number. Its own running devices stay. Never allocates.
    pub(crate) fn adopt(&mut self, fresh: &mut GroupSlot) {
        // The edit can have added or removed groups before this one: the session names a group
        // by its number in the new project, so the running slot takes the number it is built with.
        self.number = fresh.number;
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
                    (DeviceSlot::Device(device) | DeviceSlot::MidiDevice(device), _) => {
                        if let Some(samples) = device.samples() {
                            if let Some(points) = new.sample_slices.get_mut(position) {
                                samples.swap_sample_slices(points);
                            }
                            if let Some(edits) = new.sample_edits.get_mut(position) {
                                samples.swap_sample_edits(edits);
                            }
                        }
                        // A parameter that follows a lane keeps its running value until the lane
                        // sets it in the next block.
                        for (index, &value) in
                            values.iter().enumerate().filter(|&(index, _)| !lanes.covers(position, index))
                        {
                            device.set_param(index, value);
                        }
                    }
                    (DeviceSlot::Group(group), DeviceSlot::Group(new_group)) => group.adopt(new_group),
                    (DeviceSlot::Plugin(running), DeviceSlot::Plugin(fresh)) => running.adopt(fresh),
                    _ => {}
                }
            }
            std::mem::swap(&mut running.lanes, &mut new.lanes);
            std::mem::swap(&mut running.positions, &mut new.positions);
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
    Slots(std::slice::IterMut<'a, DeviceSlot>),
    Branches(std::slice::IterMut<'a, BranchRun>),
}

/// Lists walked at once at most: the chain's slots, then branches and slots for each group.
const STACK: usize = 2 + 2 * suhina_model::MAX_DEVICE_GROUP_DEPTH;

impl<'a> PluginSlots<'a> {
    pub(crate) fn new(slots: &'a mut [DeviceSlot]) -> Self {
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
                Some(Ok(DeviceSlot::Plugin(plugin))) => return Some(plugin),
                Some(Ok(DeviceSlot::Group(group))) => self.push(Frame::Branches(group.branches.iter_mut())),
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
    slots: &mut [DeviceSlot],
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
            DeviceSlot::Device(device) => {
                crate::chain::run_device(device, index, buffer, from, to, pending, lanes, context, sample_rate);
            }
            // A built-in MIDI effect (#678) changes the branch's events as in a chain.
            DeviceSlot::MidiDevice(device) => {
                crate::chain::run_midi_device(
                    device,
                    index,
                    pending,
                    span_events,
                    midi_scratch,
                    lanes,
                    crate::chain::EventSpan { from, to, context },
                );
                std::mem::swap(pending, midi_scratch);
            }
            DeviceSlot::Plugin(plugin) => {
                let last = (to - from).saturating_sub(1);
                span_events.clear();
                span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                plugin.automate(from, to);
                if let Some(span) = buffer.get_mut(from * 2..to * 2) {
                    plugin.run(span, span_events, deadline);
                }
            }
            DeviceSlot::Group(group) => group.run(buffer, from, to, pending, context, sample_rate, deadline),
        }
    }
}
