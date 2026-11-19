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
use suhina_model::{ChannelId, DeviceId, DeviceKind, Project, TrackId, walk_devices};

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

/// Where the song is as a block starts, as plug-ins are told it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongPosition {
    /// The transport runs.
    pub playing: bool,
    /// The arrangement position of the block's first frame, in frames.
    pub position: u64,
    /// The tempo, in beats per minute.
    pub tempo: f64,
    /// The position in beats (quarter notes), through the tempo map.
    pub beat: f64,
    /// The bar it is in: its number from 0, the beat it starts at, its time signature.
    pub bar: u32,
    pub bar_start: f64,
    pub numerator: u8,
    pub denominator: u8,
    /// The arrangement loop, in beats, while it is on.
    pub cycle: Option<(f64, f64)>,
    /// The same loop in seconds through the tempo map.
    pub cycle_seconds: Option<(f64, f64)>,
}

impl SongPosition {
    /// The song `frames` frames earlier at `sample_rate`: what a plug-in hears that late is from
    /// there. The tempo of the block stands for the stretch between; the bar steps back with
    /// the beat. Unchanged while the transport is stopped.
    pub fn earlier(&self, frames: u32, sample_rate: f64) -> Self {
        if !self.playing || frames == 0 || sample_rate <= 0.0 || self.tempo <= 0.0 {
            return *self;
        }
        let mut song = *self;
        song.position = song.position.saturating_sub(u64::from(frames));
        song.beat -= f64::from(frames) * song.tempo / (60.0 * sample_rate);
        let bar_beats = f64::from(song.numerator.max(1)) * 4.0 / f64::from(song.denominator.max(1));
        while song.beat < song.bar_start && song.bar > 0 {
            song.bar -= 1;
            song.bar_start -= bar_beats;
        }
        song
    }

    /// The song `frames` frames later, at this block's tempo and time signature: within the
    /// block this position starts. A loop that ends within it starts again from its start.
    fn later(&self, frames: u64, sample_rate: f64) -> Self {
        if !self.playing || frames == 0 || sample_rate <= 0.0 || self.tempo <= 0.0 {
            return *self;
        }
        let mut song = *self;
        song.position = song.position.saturating_add(frames);
        song.beat += frames as f64 * song.tempo / (60.0 * sample_rate);
        if let (Some((start, end)), Some((start_s, end_s))) = (song.cycle, song.cycle_seconds)
            && end > start
            && self.beat < end
            && song.beat >= end
        {
            song.beat = start + (song.beat - end);
            let frame_of = |seconds: f64| (seconds.max(0.0) * sample_rate).round() as u64;
            song.position = frame_of(start_s).saturating_add(song.position.saturating_sub(frame_of(end_s)));
        }
        let bar_beats = f64::from(song.numerator.max(1)) * 4.0 / f64::from(song.denominator.max(1));
        while song.beat >= song.bar_start + bar_beats {
            song.bar = song.bar.saturating_add(1);
            song.bar_start += bar_beats;
        }
        while song.beat < song.bar_start && song.bar > 0 {
            song.bar -= 1;
            song.bar_start -= bar_beats;
        }
        song
    }
}

/// How many blocks' songs a [`SongHistory`] keeps: at 32 frames a block and 48 kHz, more than
/// five seconds, past the latency a graph compensates ([`crate::MAX_LATENCY_SECONDS`]).
pub(crate) const SONG_HISTORY: usize = 8192;

/// The song as each recent block started, in the order the blocks played, so a plug-in that
/// hears its sound late is told the song of what it hears as the transport moved then: across a
/// loop's wrap, a jump, a tempo or a time signature change, where the current song stepped back
/// at the current tempo would be wrong. Allocated once; recording and reading never allocate.
pub(crate) struct SongHistory {
    /// `(frames played before the block, the song as it started)`, a ring.
    blocks: Box<[(u64, SongPosition)]>,
    /// Where the next block goes.
    next: usize,
    /// How many blocks it holds.
    len: usize,
    /// Frames played so far, the transport stopped or not.
    played: u64,
    sample_rate: f64,
}

impl SongHistory {
    /// Discards transport from before a project was suspended, keeping its storage.
    pub(crate) fn clear(&mut self) {
        self.next = 0;
        self.len = 0;
        self.played = 0;
    }

    /// An empty history. Allocates.
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            blocks: vec![(0, SongPosition::default()); SONG_HISTORY].into_boxed_slice(),
            next: 0,
            len: 0,
            played: 0,
            sample_rate: f64::from(sample_rate),
        }
    }

    /// Records the song as a block of `frames` frames starts: it is the current block until the
    /// next is recorded. Never allocates.
    pub(crate) fn record(&mut self, song: SongPosition, frames: usize) {
        let size = self.blocks.len();
        if let Some(slot) = self.blocks.get_mut(self.next) {
            *slot = (self.played, song);
        }
        self.next = (self.next + 1) % size.max(1);
        self.len = (self.len + 1).min(size);
        self.played = self.played.saturating_add(frames as u64);
    }

    /// The `i`th block kept, oldest first.
    fn block(&self, i: usize) -> Option<&(u64, SongPosition)> {
        let size = self.blocks.len().max(1);
        self.blocks.get((self.next + size - self.len + i) % size)
    }

    /// The song `frames` frames before the current block's start: the block that was playing
    /// then, moved on to that frame ([`SongPosition`]'s tempo, signature and loop of that block).
    /// Before the oldest block kept, that block stepped back ([`SongPosition::earlier`]).
    /// Never allocates.
    pub(crate) fn heard(&self, frames: u32) -> SongPosition {
        let Some(&(now, current)) = self.len.checked_sub(1).and_then(|last| self.block(last)) else {
            return SongPosition::default();
        };
        if frames == 0 {
            return current;
        }
        let Some(target) = now.checked_sub(u64::from(frames)) else {
            // Before anything was played: the first block stepped back.
            let (at, first) = self.block(0).copied().unwrap_or((now, current));
            let back = u64::from(frames).saturating_sub(now - at);
            return first.earlier(u32::try_from(back).unwrap_or(u32::MAX), self.sample_rate);
        };
        // The first block kept that started after the target; the one before it played it.
        let (mut low, mut high) = (0, self.len);
        while low < high {
            let mid = low + (high - low) / 2;
            match self.block(mid) {
                Some(&(at, _)) if at <= target => low = mid + 1,
                _ => high = mid,
            }
        }
        if let Some(&(at, song)) = low.checked_sub(1).and_then(|i| self.block(i)) {
            return song.later(target - at, self.sample_rate);
        }
        // Older than anything kept (or than the first block played): the oldest stepped back.
        let (at, oldest) = self.block(0).copied().unwrap_or((now, current));
        let back = u64::from(frames).saturating_sub(now - at);
        oldest.earlier(u32::try_from(back).unwrap_or(u32::MAX), self.sample_rate)
    }
}

impl Default for SongPosition {
    fn default() -> Self {
        Self {
            playing: false,
            position: 0,
            tempo: 120.0,
            beat: 0.0,
            bar: 0,
            bar_start: 0.0,
            numerator: 4,
            denominator: 4,
            cycle: None,
            cycle_seconds: None,
        }
    }
}

/// The audio half of a running plug-in, as the engine sees it. Implemented outside the engine
/// (the session, over `suhina-plugins`); test doubles implement it too.
pub trait PluginProcessor: Send {
    /// Clears voices and buffered audio while retaining the instance and its parameters.
    /// Called only on the control thread after its renderer has left the output.
    fn reset_audio(&mut self, _timeout: Duration) -> Result<(), String> {
        Err("this processor does not support audio reset".into())
    }
    /// The instrument softens its own transport stop before handing audio to effects.
    fn smooth_transport_stop(&self) -> bool {
        false
    }
    /// Reconfigures a retained processor between renderers, with audio fully stopped.
    /// This control operation is NOT real-time safe. Returns its new latency in frames.
    fn reconfigure(&mut self, _sample_rate: u32, _timeout: Duration) -> Result<u32, String> {
        Err("this processor does not support audio reconfiguration".into())
    }
    /// Runs `buffer`, interleaved stereo (the chain's sound so far), through the plug-in in
    /// place, with `events` at frames counted from the start of `buffer`, waiting at most
    /// `timeout`. Returns false when the plug-in did not answer in time or is gone; `buffer` is
    /// then left as it was. Never allocates, locks or does I/O beyond the block's round trip.
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, NoteEvent)], timeout: Duration) -> bool;

    /// The key (sidechain) for the next block it is handed, as many frames as that block,
    /// interleaved stereo: the plug-in's first auxiliary input hears it
    /// (`docs/spec/sidechain.md` › Plug-in buses). A processor without one ignores it. Never
    /// allocates.
    fn set_key(&mut self, key: &[f32]) {
        let _ = key;
    }

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

    /// The block handed over with [`PluginProcessor::send`], to wait for together with other
    /// plug-ins' ([`suhina_ipc::wait_any`]), while it is out and not answered. `None` (the
    /// default) for a processor that cannot tell, which is then looked at in turn as
    /// [`PluginProcessor::poll`] allows (#1256). Never allocates.
    fn watch(&self) -> Option<suhina_ipc::Watch<'_>> {
        None
    }

    /// Like [`PluginProcessor::watch`], for the block it was late with that
    /// [`PluginProcessor::ready`] waits for. Never allocates.
    fn watch_ready(&self) -> Option<suhina_ipc::Watch<'_>> {
        None
    }

    /// Parameter values for the block that [`PluginProcessor::process`] or
    /// [`PluginProcessor::send`] hands over next, at frames counted from the start of its
    /// buffer, in frame order (#109). The engine calls it before every block (with none, too);
    /// a processor keeps them for that block only. One that cannot set parameters ignores them.
    /// Never allocates.
    fn automate(&mut self, params: &[ParamEvent]) {
        let _ = params;
    }

    /// Where the song is as the next block starts ([`SongPosition`]). The engine calls it before
    /// every block; a hosted plug-in is told with the block, so its tempo-synced parts and an
    /// ARA renderer (ADR 0033) follow the song. Never allocates.
    fn transport(&mut self, song: &SongPosition) {
        let _ = song;
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

    /// How long the plug-in at `index` among the followers of the last joint block this one
    /// sent ([`PluginProcessor::send_joint`]) took to process it, in microseconds, as its host
    /// measured it, once that block is answered; `None` (the default) before that or when the
    /// processor cannot tell (#1262). Never allocates.
    fn follower_process_us(&self, index: usize) -> Option<u32> {
        let _ = index;
        None
    }

    /// The time a plug-in that followed in a joint block took to process it, in microseconds
    /// ([`PluginProcessor::follower_process_us`] of the first), so it is counted with the
    /// plug-in's own. Never allocates.
    fn followed_process_us(&mut self, process_us: u32) {
        let _ = process_us;
    }
}

/// The slots of the plug-ins that followed in a joint block this plug-in sent and that was not
/// answered in time: their processing times come with the late answer, which this plug-in's link
/// holds until its next block goes (#1262).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct LateFollowers {
    pub(crate) slots: [usize; MAX_JOINED],
    pub(crate) len: usize,
}

impl LateFollowers {
    pub(crate) fn slots(&self) -> &[usize] {
        self.slots.get(..self.len).unwrap_or_default()
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

/// The key of the plug-in device `device` of `kind` on `channel`, from its id (#763 › 1): it stays
/// the same while devices are added, removed or moved around it on the channel, identical
/// plug-ins included, so a running plug-in keeps its process and its state through edits. A
/// device moved to another channel gets a new key, and a new process from its saved state.
///
/// A device without an id yet (a project the session has not given ids, as some tests build) is
/// keyed by `nth`, how many plug-ins of `kind` come before it on the channel (counting from 0).
/// `None` for devices that are not plug-ins.
pub fn plugin_key(channel: ChannelId, kind: &DeviceKind, device: DeviceId, nth: u32) -> Option<u64> {
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
    if device.is_some() {
        eat(&[0, 1]);
        eat(&device.0.to_le_bytes());
    } else {
        eat(&[0]);
        eat(&nth.to_le_bytes());
    }
    Some(hash)
}

/// Every plug-in device in `project`, channel by channel, in order, depth-first through device
/// groups, each with its key ([`plugin_key`]): moving a plug-in into or out of a group keeps it.
pub fn plugin_devices(project: &Project) -> Vec<PluginDevice> {
    let mut list = Vec::new();
    for channel in &project.channels {
        let track = project
            .tracks
            .iter()
            .find(|t| t.channel == Some(channel.id))
            .map(|t| t.id)
            .or_else(|| (channel.role == suhina_model::ChannelRole::Master).then_some(suhina_model::TrackId::MAIN));
        let mut seen: Vec<(u64, u32)> = Vec::new();
        for (path, device) in walk_devices(&channel.devices) {
            let position = path.first().copied().unwrap_or(0);
            let Some(first) = plugin_key(channel.id, &device.kind, DeviceId::NONE, 0) else { continue };
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
            if let Some(key) = plugin_key(channel.id, &device.kind, device.id, nth) {
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
    /// The parameters of each plug-in device that hold (a switch, a list of choices), by id, as
    /// its plug-in listed them: their lanes hold each value until the next point, as they are
    /// drawn (#1205). A knob listed with steps moves along the line (#1482).
    pub(crate) steps: PluginSteps,
    pub(crate) processors: HashMap<u64, Box<dyn PluginProcessor>>,
    /// Processors retained from a closed renderer can still hold its sounding notes.
    pub(crate) carried: bool,
}

impl Plugins {
    /// Reconfigures all retained processors before building a renderer at a new rate.
    /// Never called from rendering. Failure aborts opening the new renderer: mixing
    /// processors configured at different rates would produce incorrect sound.
    pub fn reconfigure(&mut self, sample_rate: u32, timeout: Duration) -> Result<(), crate::ControlError> {
        let deadline = Instant::now() + timeout;
        for (key, processor) in &mut self.processors {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(crate::ControlError::Plugin("plug-in audio reconfiguration timed out".into()));
            }
            let latency = processor
                .reconfigure(sample_rate, left)
                .map_err(|why| crate::ControlError::Plugin(format!("plug-in {key}: {why}")))?;
            if let Some(info) = self.info.get_mut(key) {
                info.latency = latency;
            }
        }
        Ok(())
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Gives the plug-in device `key` a slot, described by `info`; it passes the sound through
    /// (or is silent, for an instrument) until a processor arrives.
    pub fn set_info(&mut self, key: u64, info: PluginSlotInfo) {
        self.info.insert(key, info);
    }

    /// The parameters of the plug-in device `key` whose lanes hold each value, by id
    /// ([`Plugins::steps`]).
    pub fn set_steps(&mut self, key: u64, ids: std::sync::Arc<[u32]>) {
        self.steps.insert(key, ids);
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

/// The parameters of each plug-in device whose lanes hold each value until the next point (a
/// switch, a list of choices), by the device's key and the parameters' ids ([`Plugins::steps`]).
pub type PluginSteps = HashMap<u64, std::sync::Arc<[u32]>>;

/// A plug-in device's place in a chain.
pub(crate) struct PluginSlot {
    pub(crate) key: u64,
    pub(crate) processor: Option<Box<dyn PluginProcessor>>,
    pub(crate) instrument: bool,
    pub(crate) latency: u32,
    pub(crate) enabled: bool,
    /// Round trips it did not answer in time or refused; tests read it.
    pub(crate) missed: u64,
    /// Blocks the host refused outright (still busy with an earlier one), of `missed`.
    pub(crate) busy: u64,
    /// The longest trip it answered within a real-time deadline, in microseconds; for the
    /// trace's health line.
    pub(crate) longest_us: u32,
    /// When the block out was sent, for [`PluginSlot::longest_us`].
    pub(crate) sent_at: Option<Instant>,
    /// The parameter values it sends: set ones and automation.
    pub(crate) params: SlotParams,
    /// The followers of its last joint block, while that block's late answer has not given
    /// their processing times yet. Boxed, as it is seldom used, and made with the slot, so the
    /// audio thread only reads and writes it in place.
    pub(crate) late_followers: Box<LateFollowers>,
    /// It reads a key (sidechain): its blocks go on their own, never joined with another
    /// plug-in's, since a joint block carries one plug-in's input.
    pub(crate) keyed: bool,
    /// How late the sound it gets is, in frames, from the devices before it in its chain: the
    /// song position it is told is moved back by this and the chain's input delay, so a plug-in
    /// that follows the song (a tempo-synced shape, an arpeggio) lines up with what it hears.
    pub(crate) song_delay: u32,
}

impl PluginSlot {
    /// Takes the settings of `fresh`, the slot built for the same plug-in after an edit, and its
    /// processor when one reached it (a restart), which then gets the set values again. Never
    /// allocates.
    pub(crate) fn adopt(&mut self, fresh: &mut PluginSlot) {
        self.enabled = fresh.enabled;
        self.instrument = fresh.instrument;
        self.latency = fresh.latency;
        // The devices before it may report another latency now (a plug-in tells its own once it
        // has loaded): it hears the song as late as the new graph says.
        self.song_delay = fresh.song_delay;
        self.params.adopt(&mut fresh.params);
        // The slots it remembered may have moved with the edit.
        self.late_followers.len = 0;
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
    /// span) until `deadline`, or [`OFFLINE_TIMEOUT`] without one. A disabled effect passes the
    /// sound through; a disabled instrument is silent, as when it misses a block.
    pub(crate) fn run(&mut self, span: &mut [f32], events: &[(usize, NoteEvent)], deadline: Option<Instant>) {
        if !self.enabled || span.is_empty() {
            if !self.enabled && self.instrument {
                span.fill(0.0);
            }
            return;
        }
        let timeout = deadline.map_or(OFFLINE_TIMEOUT, |d| d.saturating_duration_since(crate::clock::now()));
        let started = crate::clock::now();
        let answered = match self.processor.as_mut() {
            Some(processor) => processor.process(span, events, timeout),
            None => false,
        };
        if deadline.is_some() && answered {
            self.longest_us = self.longest_us.max(u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX));
        }
        self.sent_at = None;
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
        if deadline.is_some() {
            self.sent_at = match sent {
                Sent::Out => Some(crate::clock::now()),
                Sent::Refused => {
                    self.busy += 1;
                    None
                }
                Sent::Unsupported => None,
            };
        }
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
        let sent_at = deadline.is_some().then(|| self.sent_at.take()).flatten();
        let timeout = deadline.map_or(OFFLINE_TIMEOUT, |d| d.saturating_duration_since(crate::clock::now()));
        let answered = self.processor.as_mut().is_some_and(|processor| processor.wait(span, timeout));
        if let Some(sent_at) = sent_at
            && answered
        {
            self.longest_us = self.longest_us.max(u32::try_from(sent_at.elapsed().as_micros()).unwrap_or(u32::MAX));
        }
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

    /// Its block out, or with `behind` the block it was late with, to wait for together with
    /// others' ([`PluginProcessor::watch`]). Never allocates.
    pub(crate) fn watch(&self, behind: bool) -> Option<suhina_ipc::Watch<'_>> {
        let processor = self.processor.as_ref()?;
        if behind { processor.watch_ready() } else { processor.watch() }
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

    /// Hands the plug-in the key (sidechain) for the block it is handed next. Never allocates.
    pub(crate) fn set_key(&mut self, key: &[f32]) {
        self.keyed = true;
        if let Some(processor) = self.processor.as_mut() {
            processor.set_key(key);
        }
    }

    /// Where the plug-in runs, when it is enabled and can take part in a joint round trip now.
    pub(crate) fn joint(&self) -> Option<Joint> {
        if !self.enabled || self.keyed {
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
        // The lead slot's round trip is recorded as a plain one's is: the wait that follows is
        // timed (`finish`), and a refused one counts as busy.
        if deadline.is_some() {
            self.sent_at = match sent {
                Sent::Out => Some(crate::clock::now()),
                Sent::Refused => {
                    self.busy += 1;
                    None
                }
                Sent::Unsupported => None,
            };
        }
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
    /// Drops sound from before a project was suspended, keeping its allocation.
    pub(crate) fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.at = 0;
    }

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
        let key = |channel: u32, kind: &DeviceKind, id: u32, nth: u32| {
            plugin_key(ChannelId(channel), kind, suhina_model::DeviceId(id), nth)
        };
        let a = key(1, &vst3, 5, 0);
        assert!(a.is_some());
        assert_eq!(a, key(1, &vst3, 5, 3), "an id makes the place irrelevant");
        assert_ne!(a, key(1, &vst3, 6, 0));
        assert_ne!(a, key(2, &vst3, 5, 0));
        assert_ne!(a, key(1, &DeviceKind::Clap { id: "ABC".into() }, 5, 0));
        assert_ne!(key(1, &vst3, 0, 0), key(1, &vst3, 0, 1), "without an id, by place");
        assert_ne!(key(1, &vst3, 0, 1), key(1, &vst3, 1, 0), "an id is never taken for a place");
        assert_eq!(key(1, &DeviceKind::Builtin("utility".into()), 5, 0), None);

        let mut project = crate::chain_tests::empty_project();
        let with_id = |kind: DeviceKind, id: u32| Device { id: suhina_model::DeviceId(id), ..plug(kind) };
        project.channels[0].devices = vec![
            with_id(vst3.clone(), 1),
            with_id(DeviceKind::Builtin("utility".into()), 2),
            with_id(vst3.clone(), 3),
            with_id(DeviceKind::Vst2 { id: 7 }, 4),
        ];
        let found = plugin_devices(&project);
        assert_eq!(found.iter().map(|d| d.position).collect::<Vec<_>>(), [0, 2, 3]);
        assert_eq!(Some(found[1].key), key(1, &vst3, 3, 0));
        // A device added in front of them changes no key.
        project.channels[0].devices.insert(0, with_id(DeviceKind::Builtin("utility".into()), 5));
        let after: Vec<u64> = plugin_devices(&project).iter().map(|d| d.key).collect();
        assert_eq!(after, found.iter().map(|d| d.key).collect::<Vec<_>>());
        // The first of the two identical plug-ins is removed: the second keeps its own key, so
        // its own process, not the removed one's.
        project.channels[0].devices.remove(1);
        let after: Vec<u64> = plugin_devices(&project).iter().map(|d| d.key).collect();
        assert_eq!(after, [found[1].key, found[2].key]);
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
