// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in devices in a track's chain (ADR 0006).
//!
//! The engine never loads a plug-in and does not know how one is hosted: a plug-in device's slot
//! in a [`Chain`](crate::chain::Chain) holds a [`PluginProcessor`], the audio half of a plug-in
//! running in a host process of its own, which the session starts off the audio thread and
//! hands over. A slot runs one round trip per block with the block's notes at their frames, and
//! waits only until the block's deadline: a plug-in that misses it, or whose host has crashed,
//! leaves the chain's sound as it was (an effect) or silent (an instrument) for that block.
//!
//! Consecutive plug-ins of a chain that run in the same host process share one round trip a
//! block (ADR 0006, decision 2): the first takes the block with the others named after it
//! ([`PluginProcessor::send_joint`]), its host runs them all in turn, and the others are told how
//! it went ([`PluginProcessor::followed`]). The sound is the same as one round trip each.
//!
//! Plug-in devices are told apart by a [`plugin_key`], so a processor finds its slot again in
//! every graph built after an edit, and one that arrives while no graph has its slot waits for
//! one.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use suhina_device_api::NoteEvent;
use suhina_model::{ChannelId, DeviceKind, Project, TrackId, walk_devices};

use crate::plugin_params::{ParamEvent, SlotParams};

/// How long a slot waits for a block when rendering offline, where there is no deadline: long
/// enough for any plug-in that still works.
pub const OFFLINE_TIMEOUT: Duration = Duration::from_secs(10);
/// Share of a block's playing time the plug-ins of a block may take together before the engine
/// stops waiting for them; the rest is for the engine itself and the audio device.
pub(crate) const DEADLINE_SHARE: f64 = 0.6;

/// What became of a block handed to a plug-in with [`PluginProcessor::send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    /// It is with the plug-in; [`PluginProcessor::wait`] takes the answer.
    Out,
    /// The plug-in could not take it (still busy with an earlier block, past the patience it
    /// was given): this round trip is a miss and the block is lost to the plug-in, so an effect
    /// passes the sound through and an instrument is silent for it.
    Refused,
    /// The processor does not split a block into sending and waiting; `process` runs it.
    Unsupported,
}

/// Where a plug-in runs, for running several in one round trip: plug-ins with the same
/// `process` live in one host process, where `member` is this one's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Joint {
    pub process: u64,
    pub member: u32,
}

/// Most plug-ins one joint round trip runs after the first.
pub const MAX_JOINED: usize = 15;

/// How a joint block went, for a plug-in that followed the first in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followed {
    /// Answered in time: the chain's sound holds its output.
    Answered,
    /// Sent, but not answered in time.
    Late,
    /// Not sent: the first plug-in's host was still busy with an earlier block.
    Refused,
}

/// The audio half of a running plug-in, as the engine sees it. Implemented outside the engine
/// (the session, over `suhina-plugins`); test doubles implement it too.
pub trait PluginProcessor: Send {
    /// Runs `buffer`, interleaved stereo (the chain's sound so far), through the plug-in in
    /// place, with `events` at frames counted from the start of `buffer`, waiting at most
    /// `timeout`. Returns false when the plug-in did not answer in time or is gone; `buffer` is
    /// then left as it was. Never allocates, locks or does I/O beyond the block's round trip.
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, NoteEvent)], timeout: Duration) -> bool;

    /// The first half of [`PluginProcessor::process`]: hands `buffer` and `events` to the
    /// plug-in without waiting, so the plug-ins of several chains run at the same time and a
    /// block's deadline is shared by the slowest rather than by all of them in turn. A
    /// processor that cannot split a block leaves this saying [`Sent::Unsupported`], and the
    /// chain runs the block with `process`; one that can but whose plug-in cannot take the
    /// block (it has not answered an earlier one) within `patience` says [`Sent::Refused`], and
    /// the block is a miss, counted once, without trying again. `patience` is zero in real time,
    /// where waiting would spend the deadline every chain shares, and [`OFFLINE_TIMEOUT`] in an
    /// offline render, which waits for every block (ADR 0006, decision 4).
    fn send(&mut self, buffer: &[f32], events: &[(usize, NoteEvent)], patience: Duration) -> Sent {
        let _ = (buffer, events, patience);
        Sent::Unsupported
    }

    /// The second half: waits at most `timeout` for the block handed over with
    /// [`PluginProcessor::send`] and puts the result in `buffer`. Returns false when the plug-in
    /// did not answer in time; `buffer` is then left as it was.
    fn wait(&mut self, buffer: &mut [f32], timeout: Duration) -> bool {
        let _ = (buffer, timeout);
        false
    }

    /// Whether the answer to the block handed over with [`PluginProcessor::send`] is in,
    /// waiting for it at most until `until` (a time already past only looks), without taking
    /// it: [`PluginProcessor::wait`] does, without waiting, after `true`. The engine looks at
    /// the chains whose blocks are out in turn and goes on with each as its own plug-in answers
    /// (#151). A processor that cannot tell says `true` (the default), and is waited for with
    /// `wait` at once. Never allocates; never waits past `until`.
    fn poll(&mut self, until: Instant) -> bool {
        let _ = until;
        true
    }

    /// Whether a block can go to the plug-in now, waiting at most until `until` (a time already
    /// past only looks) for it to finish a block it was late with: in real time a plug-in that
    /// missed one block's deadline over a burst of work (the notes of a start, a stop's
    /// all-notes-off) is often done early in the next block, which is then sent as soon as it is
    /// rather than refused at once and lost too (#506). A processor that cannot tell says `true`
    /// (the default), and its block is sent, or refused, at once. Never allocates; never waits
    /// past `until`.
    fn ready(&mut self, until: Instant) -> bool {
        let _ = until;
        true
    }

    /// Parameter values for the block that [`PluginProcessor::process`] or
    /// [`PluginProcessor::send`] hands over next, at frames counted from the start of its
    /// buffer, in frame order (#109). The engine calls it before every block (with none, too);
    /// a processor keeps them for that block only. One that cannot set parameters ignores them.
    /// Never allocates.
    fn automate(&mut self, params: &[ParamEvent]) {
        let _ = params;
    }

    /// Where the plug-in runs while it can take part in a joint round trip now; `None` (the
    /// default) when it cannot, and it runs on its own.
    fn joint(&self) -> Option<Joint> {
        None
    }

    /// [`PluginProcessor::send`] for a block that the plug-ins `followers` (their
    /// [`Joint::member`]s, all in this one's process) run after this one, in order, with the
    /// same events, in the same round trip; [`PluginProcessor::wait`] then takes the last one's
    /// output. A processor that cannot says [`Sent::Unsupported`] (the default) and the chain
    /// runs each plug-in on its own.
    fn send_joint(
        &mut self,
        buffer: &[f32],
        events: &[(usize, NoteEvent)],
        followers: &[u32],
        patience: Duration,
    ) -> Sent {
        let _ = (buffer, events, followers, patience);
        Sent::Unsupported
    }

    /// Tells a plug-in that followed in a joint round trip how it went, with the block's
    /// `events` (which a refused block did not deliver). Never allocates.
    fn followed(&mut self, events: &[(usize, NoteEvent)], how: Followed) {
        let _ = (events, how);
    }
}

/// What the engine needs to know about a plug-in device to place it in a chain: whether it
/// plays notes and the delay it adds, in frames (0 until it has loaded and said so).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginSlotInfo {
    pub instrument: bool,
    pub latency: u32,
}

/// A plug-in device in a project: where it is and the key its processor goes by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDevice {
    pub key: u64,
    pub channel: ChannelId,
    /// The device's index among the channel's devices; for a device inside a device group, the
    /// index of the outermost group.
    pub position: usize,
    /// The device's path on the channel: its index, then branch and index for each device group
    /// it is in (`suhina_model::device_at`).
    pub path: Vec<usize>,
    /// The track whose channel it is on, if any.
    pub track: Option<TrackId>,
}

/// The key of the `nth` plug-in device of `kind` (counting from 0) on `channel`: it stays the
/// same while devices are added, removed or moved around it, so a running plug-in keeps its
/// slot through edits. `None` for devices that are not plug-ins.
pub fn plugin_key(channel: ChannelId, kind: &DeviceKind, nth: u32) -> Option<u64> {
    let (format, id): (u8, String) = match kind {
        DeviceKind::Clap { id } => (1, id.clone()),
        DeviceKind::Vst3 { id } => (2, id.clone()),
        DeviceKind::Vst2 { id } => (3, id.to_string()),
        DeviceKind::Builtin(_) | DeviceKind::Sd | DeviceKind::Foreign { .. } | DeviceKind::Group(_) => return None,
    };
    // FNV-1a: stable across runs and builds, unlike the standard library's hasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(&channel.0.to_le_bytes());
    eat(&[format]);
    eat(id.as_bytes());
    eat(&[0]);
    eat(&nth.to_le_bytes());
    Some(hash)
}

/// Every plug-in device in `project`, channel by channel, in order: depth-first through device
/// groups, so "the nth of the same plug-in" counts the same way inside groups as outside, and
/// moving a plug-in into or out of a group keeps its key while the order of identical plug-ins
/// stays (`docs/spec/device-groups.md`).
pub fn plugin_devices(project: &Project) -> Vec<PluginDevice> {
    let mut list = Vec::new();
    for channel in &project.channels {
        let track = project.tracks.iter().find(|t| t.channel == Some(channel.id)).map(|t| t.id);
        let mut seen: Vec<(u64, u32)> = Vec::new();
        for (path, device) in walk_devices(&channel.devices) {
            let position = path.first().copied().unwrap_or(0);
            let Some(first) = plugin_key(channel.id, &device.kind, 0) else { continue };
            let nth = match seen.iter_mut().find(|(k, _)| *k == first) {
                Some((_, count)) => {
                    *count += 1;
                    *count
                }
                None => {
                    seen.push((first, 0));
                    0
                }
            };
            if let Some(key) = plugin_key(channel.id, &device.kind, nth) {
                list.push(PluginDevice { key, channel: channel.id, position, path, track });
            }
        }
    }
    list
}

/// The plug-ins a renderer is built with: what is known about each plug-in device (devices
/// without an entry, such as missing plug-ins, have no slot and pass the sound through) and the
/// processors of the ones that run.
#[derive(Default)]
pub struct Plugins {
    pub(crate) info: HashMap<u64, PluginSlotInfo>,
    pub(crate) processors: HashMap<u64, Box<dyn PluginProcessor>>,
}

impl Plugins {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gives the plug-in device `key` a slot, described by `info`; it passes the sound through
    /// (or is silent, for an instrument) until a processor arrives.
    pub fn set_info(&mut self, key: u64, info: PluginSlotInfo) {
        self.info.insert(key, info);
    }

    /// The plug-in device `key`, running in `processor`.
    pub fn insert(&mut self, key: u64, info: PluginSlotInfo, processor: Box<dyn PluginProcessor>) {
        self.info.insert(key, info);
        self.processors.insert(key, processor);
    }

    pub fn info(&self) -> &HashMap<u64, PluginSlotInfo> {
        &self.info
    }
}

/// A plug-in device's place in a chain.
pub(crate) struct PluginSlot {
    pub(crate) key: u64,
    pub(crate) processor: Option<Box<dyn PluginProcessor>>,
    pub(crate) instrument: bool,
    pub(crate) latency: u32,
    pub(crate) enabled: bool,
    /// Round trips it did not answer in time or refused; tests read it.
    pub(crate) missed: u64,
    /// The parameter values it sends: set ones and automation.
    pub(crate) params: SlotParams,
}

impl PluginSlot {
    /// Takes the settings of `fresh`, the slot built for the same plug-in after an edit, and its
    /// processor when one reached it (a restart), which then gets the set values again. Never
    /// allocates.
    pub(crate) fn adopt(&mut self, fresh: &mut PluginSlot) {
        self.enabled = fresh.enabled;
        self.instrument = fresh.instrument;
        self.latency = fresh.latency;
        self.params.adopt(&mut fresh.params);
        if fresh.processor.is_some() {
            std::mem::swap(&mut self.processor, &mut fresh.processor);
            self.params.restart();
        }
    }

    /// Hands the processor the parameter events of the block's frames `from..to`, the span it
    /// runs next; whether there are any. Never allocates.
    pub(crate) fn automate(&mut self, from: usize, to: usize) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(processor) = self.processor.as_mut() else { return false };
        let params = self.params.span(from, to);
        processor.automate(params);
        !params.is_empty()
    }

    /// Runs `span` through the plug-in with `events` (frames counted from the start of the
    /// span) until `deadline`, or [`OFFLINE_TIMEOUT`] without one. A disabled slot passes the
    /// sound through; one that misses passes it through too, or is silent for an instrument.
    pub(crate) fn run(&mut self, span: &mut [f32], events: &[(usize, NoteEvent)], deadline: Option<Instant>) {
        if !self.enabled || span.is_empty() {
            return;
        }
        let timeout = deadline.map_or(OFFLINE_TIMEOUT, |d| d.saturating_duration_since(Instant::now()));
        let answered = match self.processor.as_mut() {
            Some(processor) => processor.process(span, events, timeout),
            None => false,
        };
        if !answered {
            if self.processor.is_some() {
                self.missed += 1;
            }
            if self.instrument {
                span.fill(0.0);
            }
        }
    }

    /// Hands `span` with `events` to the plug-in without waiting for its answer
    /// ([`PluginProcessor::send`]); offline (`deadline` `None`) a host still busy with an earlier
    /// block is waited for, up to [`OFFLINE_TIMEOUT`]. [`Sent::Out`]: [`PluginSlot::finish`]
    /// must follow. [`Sent::Refused`]: this round trip is already a miss (the sound passes, or an
    /// instrument is silent) and must not be run again. [`Sent::Unsupported`] (also a disabled
    /// or empty slot, or none running): the chain runs the slot with [`PluginSlot::run`].
    pub(crate) fn send(&mut self, span: &mut [f32], events: &[(usize, NoteEvent)], deadline: Option<Instant>) -> Sent {
        if !self.enabled || span.is_empty() {
            return Sent::Unsupported;
        }
        let Some(processor) = self.processor.as_mut() else { return Sent::Unsupported };
        let patience = if deadline.is_some() { Duration::ZERO } else { OFFLINE_TIMEOUT };
        let sent = processor.send(span, events, patience);
        if sent == Sent::Refused {
            self.missed += 1;
            if self.instrument {
                span.fill(0.0);
            }
        }
        sent
    }

    /// Waits for the block handed over with [`PluginSlot::send`] until `deadline` and puts the
    /// result in `span`; a miss passes the sound through, or is silent for an instrument.
    /// Returns whether it was answered.
    pub(crate) fn finish(&mut self, span: &mut [f32], deadline: Option<Instant>) -> bool {
        let timeout = deadline.map_or(OFFLINE_TIMEOUT, |d| d.saturating_duration_since(Instant::now()));
        let answered = self.processor.as_mut().is_some_and(|processor| processor.wait(span, timeout));
        if !answered {
            if self.processor.is_some() {
                self.missed += 1;
            }
            if self.instrument {
                span.fill(0.0);
            }
        }
        answered
    }

    /// Whether the answer to the block handed over with [`PluginSlot::send`] is in, waiting at
    /// most until `until` ([`PluginProcessor::poll`]); true without a processor, as there is
    /// nothing to wait for. Never allocates.
    pub(crate) fn poll(&mut self, until: Instant) -> bool {
        self.processor.as_mut().is_none_or(|processor| processor.poll(until))
    }

    /// Whether a block can go to the plug-in now, waiting at most until `until` for one it was
    /// late with ([`PluginProcessor::ready`]); true when disabled or without a processor, as
    /// nothing will be sent. Never allocates.
    pub(crate) fn ready(&mut self, until: Instant) -> bool {
        !self.enabled || self.processor.as_mut().is_none_or(|processor| processor.ready(until))
    }

    /// Whether it has parameter events for the block's frames `from..to`, without handing them
    /// over. Never allocates.
    pub(crate) fn has_params(&mut self, from: usize, to: usize) -> bool {
        self.enabled && !self.params.span(from, to).is_empty()
    }

    /// Where the plug-in runs, when it is enabled and can take part in a joint round trip now.
    pub(crate) fn joint(&self) -> Option<Joint> {
        if !self.enabled {
            return None;
        }
        self.processor.as_ref()?.joint()
    }

    /// [`PluginSlot::send`] for a joint block that `followers` run after this plug-in
    /// ([`PluginProcessor::send_joint`]). [`Sent::Unsupported`]: nothing was sent, and the
    /// plug-ins run on their own.
    pub(crate) fn send_joint(
        &mut self,
        span: &mut [f32],
        events: &[(usize, NoteEvent)],
        followers: &[u32],
        deadline: Option<Instant>,
    ) -> Sent {
        if !self.enabled || span.is_empty() {
            return Sent::Unsupported;
        }
        let Some(processor) = self.processor.as_mut() else { return Sent::Unsupported };
        let patience = if deadline.is_some() { Duration::ZERO } else { OFFLINE_TIMEOUT };
        let sent = processor.send_joint(span, events, followers, patience);
        if sent == Sent::Refused {
            self.missed += 1;
            if self.instrument {
                span.fill(0.0);
            }
        }
        sent
    }

    /// Takes in how a joint block it followed in went: one that was not answered is a miss for
    /// it too, so the sound passes it, or it is silent for an instrument, as when every plug-in
    /// of the joint block misses on its own.
    pub(crate) fn followed(&mut self, span: &mut [f32], events: &[(usize, NoteEvent)], how: Followed) {
        if let Some(processor) = self.processor.as_mut() {
            processor.followed(events, how);
        }
        if how != Followed::Answered {
            self.missed += 1;
            if self.instrument {
                span.fill(0.0);
            }
        }
    }
}

/// A delay line for latency compensation: interleaved stereo, `frames` long, allocated when it
/// is made.
pub(crate) struct Delay {
    buffer: Vec<f32>,
    at: usize,
}

impl Delay {
    pub(crate) fn new(frames: usize) -> Self {
        Self { buffer: vec![0.0; frames * 2], at: 0 }
    }

    /// Delays `samples` (interleaved stereo) in place. Never allocates.
    pub(crate) fn run(&mut self, samples: &mut [f32]) {
        if self.buffer.is_empty() {
            return;
        }
        for frame in samples.as_chunks_mut::<2>().0 {
            if let Some(stored) =
                self.buffer.get_mut(self.at..self.at + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok())
            {
                std::mem::swap(stored, frame);
            }
            self.at += 2;
            if self.at >= self.buffer.len() {
                self.at = 0;
            }
        }
    }

    /// Takes over what `old` holds when it is as long, so a graph swap that keeps the delay
    /// does not drop the sound in it. Never allocates.
    pub(crate) fn adopt(&mut self, old: &mut Delay) {
        if old.buffer.len() == self.buffer.len() {
            std::mem::swap(&mut self.buffer, &mut old.buffer);
            self.at = old.at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::Device;

    fn plug(kind: DeviceKind) -> Device {
        Device::new("P", kind)
    }

    #[test]
    fn keys_follow_the_plug_in_not_its_position() {
        let vst3 = DeviceKind::Vst3 { id: "ABC".into() };
        let a = plugin_key(ChannelId(1), &vst3, 0);
        assert!(a.is_some());
        assert_eq!(a, plugin_key(ChannelId(1), &vst3, 0));
        assert_ne!(a, plugin_key(ChannelId(1), &vst3, 1));
        assert_ne!(a, plugin_key(ChannelId(2), &vst3, 0));
        assert_ne!(a, plugin_key(ChannelId(1), &DeviceKind::Clap { id: "ABC".into() }, 0));
        assert_eq!(plugin_key(ChannelId(1), &DeviceKind::Builtin("utility".into()), 0), None);

        let mut project = crate::chain_tests::empty_project();
        project.channels[0].devices = vec![
            plug(vst3.clone()),
            plug(DeviceKind::Builtin("utility".into())),
            plug(vst3.clone()),
            plug(DeviceKind::Vst2 { id: 7 }),
        ];
        let found = plugin_devices(&project);
        assert_eq!(found.iter().map(|d| d.position).collect::<Vec<_>>(), [0, 2, 3]);
        assert_eq!(found[0].key, a.unwrap_or_default());
        assert_eq!(Some(found[1].key), plugin_key(ChannelId(1), &vst3, 1));
        // A device added in front of them changes no key.
        project.channels[0].devices.insert(0, plug(DeviceKind::Builtin("utility".into())));
        let after: Vec<u64> = plugin_devices(&project).iter().map(|d| d.key).collect();
        assert_eq!(after, found.iter().map(|d| d.key).collect::<Vec<_>>());
    }

    #[test]
    fn a_plug_in_keeps_its_key_when_it_moves_into_a_device_group() {
        use suhina_model::{Branch, DeviceGroup};
        let vst3 = DeviceKind::Vst3 { id: "ABC".into() };
        let mut project = crate::chain_tests::empty_project();
        project.channels[0].devices =
            vec![plug(vst3.clone()), plug(vst3.clone()), plug(DeviceKind::Clap { id: "c".into() })];
        let before: Vec<u64> = plugin_devices(&project).iter().map(|d| d.key).collect();
        // The second VST3 and the CLAP go into a group of two branches after the first VST3.
        let (first, second, clap) = (plug(vst3.clone()), plug(vst3), plug(DeviceKind::Clap { id: "c".into() }));
        let branches = vec![
            Branch { devices: vec![second], ..Branch::new("A") },
            Branch { devices: vec![clap], ..Branch::new("B") },
        ];
        project.channels[0].devices = vec![first, Device::group("G", DeviceGroup::with_branches(branches))];
        let found = plugin_devices(&project);
        assert_eq!(found.iter().map(|d| d.key).collect::<Vec<_>>(), before);
        assert_eq!(found.iter().map(|d| d.path.clone()).collect::<Vec<_>>(), [vec![0], vec![1, 0, 0], vec![1, 1, 0]]);
        assert_eq!(found.iter().map(|d| d.position).collect::<Vec<_>>(), [0, 1, 1]);
    }

    #[test]
    fn a_delay_line_delays_and_carries_over() {
        let mut delay = Delay::new(2);
        let mut block = [1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        delay.run(&mut block);
        assert_eq!(block, [0.0, 0.0, 0.0, 0.0, 1.0, -1.0]);
        let mut fresh = Delay::new(2);
        fresh.adopt(&mut delay);
        let mut next = [0.0; 4];
        fresh.run(&mut next);
        assert_eq!(next, [2.0, -2.0, 3.0, -3.0]);
        let mut none = Delay::new(0);
        let mut same = [5.0, 6.0];
        none.run(&mut same);
        assert_eq!(same, [5.0, 6.0]);
    }
}
