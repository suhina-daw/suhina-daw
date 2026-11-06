// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live input ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)): notes and
//! performance controls (a MIDI controller, the pitch bend wheel, channel pressure) played on a
//! MIDI controller, heard through the instrument of every armed track, whether the transport
//! runs or not.
//!
//! The MIDI driver's thread sends notes and controls over a lock-free queue, each with where it
//! came from (a port and a channel, [`LiveSource`]) and, when the driver stamped it, when it was
//! played on the live clock ([`live_now`]); the render thread takes them at the start of each
//! block and plays them on the chains of the armed tracks whose input ([`LiveFilter`]) takes
//! that source, the way the preview voice plays its notes (ADR 0011). A stamped note is played a
//! fixed delay after its time ([`Renderer::set_live_clock`](crate::Renderer::set_live_clock)), at
//! the frame where that falls, so notes keep the spacing they were played with whatever the block
//! size (#269); a note without a time plays at the block's first frame. A controller, pitch bend
//! or channel pressure message (#812) reaches an armed track's instrument the same way, as a
//! [`suhina_device_api::NoteEvent`], but carries no key state of its own.
//! Which tracks are armed, and what each hears, comes from the controller
//! ([`crate::LaunchCommand::SetArmed`]) and outlives graph swaps. The keys held and the sustain
//! pedal are kept per source, so one controller's pedal does not hold another's notes. Nothing
//! here allocates, locks or frees on the render thread.

use std::sync::OnceLock;
use std::time::Instant;

use rtrb::{Consumer, Producer, RingBuffer};
use suhina_device_api::NoteEvent;
use suhina_model::TrackId;

use crate::chain::Chain;
use crate::record::Recorder;

/// Notes waiting for the render thread: more than a hand plays within one block of any size.
pub const LIVE_QUEUE: usize = 1024;
/// Tracks that can be armed at once; arming more leaves the rest unarmed.
pub const MAX_ARMED: usize = 64;
/// Sources (a port and a channel) whose keys and pedal are followed at once; past that, the one
/// with nothing sounding is reused.
pub const MAX_SOURCES: usize = 16;
/// A live note's end: it sounds until its key is released.
const UNTIL_RELEASED: u64 = u64::MAX - 1;
/// Stamped notes waiting for their frame in a later block: more than a hand plays within the
/// delay. Past that, a note plays at once.
const MAX_WAITING: usize = 256;
/// Of those, what controllers, pitch bend and pressure may take, each in a place of its own;
/// past that, a new value takes the place of the one before it. A stream of them never leaves a
/// note without a place: a note that played at once could overtake the one before it (a release
/// before its key), and hang.
const MAX_WAITING_CONTROLS: usize = MAX_WAITING / 2;
/// How much of the difference between the render thread's clock and the steady block cadence
/// is taken in each block: enough to follow a device clock that drifts, too little to follow
/// the render thread's own jitter.
const CLOCK_PULL: f64 = 1.0 / 64.0;

/// Now, on the clock live notes are stamped with: microseconds since this process first asked,
/// never 0 (0 is a note without a time). Reading it does not allocate, lock or block.
pub fn live_now() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX).saturating_add(1)
}

/// Turns a MIDI driver's timestamps (microseconds on a clock of the driver's own) into times on
/// the live clock ([`live_now`]), for one port: the driver's clock is placed where the shortest
/// gap from a stamp to its arrival puts it, since a message can arrive late but never early.
/// The gap is let grow slowly (about 120 parts per million of the time between messages) to
/// follow a driver clock that runs slow, and a stamp that goes back starts it again.
#[derive(Debug, Clone, Copy, Default)]
pub struct StampClock {
    /// Live-clock time minus driver time, and the last stamp.
    offset: Option<(i128, u64)>,
}

impl StampClock {
    /// The live-clock time of a message the driver stamped `stamp` and that arrived at
    /// `arrived` (live clock); never after it arrived, and never 0.
    pub fn time(&mut self, stamp: u64, arrived: u64) -> u64 {
        let gap = i128::from(arrived) - i128::from(stamp);
        let offset = match self.offset {
            Some((offset, last)) if stamp >= last => {
                let creep = i128::from((stamp - last) / 8192);
                gap.min(offset.saturating_add(creep))
            }
            _ => gap,
        };
        self.offset = Some((offset, stamp));
        let time = (i128::from(stamp) + offset).clamp(1, i128::from(arrived.max(1)));
        u64::try_from(time).unwrap_or(arrived)
    }
}

/// What a MIDI controller played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LiveEvent {
    /// A key went down; `velocity` is from 0 to 1 (a note on of velocity 0 is a [`LiveEvent::NoteOff`]).
    NoteOn {
        key: u8,
        velocity: f32,
    },
    NoteOff {
        key: u8,
    },
    /// The sustain pedal went down or up: while it is down, released keys keep sounding until
    /// it comes up.
    Sustain {
        on: bool,
    },
    /// A MIDI controller other than the sustain pedal moved: `number` 0 to 127, `value` 0 to 1
    /// (#812).
    Controller {
        number: u8,
        value: f32,
    },
    /// The pitch bend wheel moved, from -1 (full down) through 0 to 1 (full up) (#812).
    PitchBend(f32),
    /// Channel pressure (aftertouch) moved, from 0 to 1 (#812).
    Pressure(f32),
    /// Every key the controller holds goes up (a closed port, an all-notes-off message).
    AllNotesOff,
}

/// Where a live note came from: a port, by [`port_id`] of its name (0 when unknown), and a
/// MIDI channel, 0 to 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveSource {
    pub port: u64,
    pub channel: u8,
}

/// The id of the MIDI port called `name` in a [`LiveSource`] and a [`LiveFilter`]: the same
/// name gives the same id in the MIDI layer and in the session, with no table to share. Never 0.
pub fn port_id(name: &str) -> u64 {
    // FNV-1a, 64 bits.
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    hash.max(1)
}

/// Which ports a track hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LivePorts {
    /// Every port.
    #[default]
    All,
    /// The port with this [`port_id`].
    One(u64),
    /// None: the track plays nothing from a controller.
    Nothing,
}

/// What an armed track hears: its ports and its channel (0 to 15, or every one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveFilter {
    pub ports: LivePorts,
    pub channel: Option<u8>,
}

impl LiveFilter {
    /// Whether a note from `source` plays on a track with this input.
    pub fn takes(&self, source: LiveSource) -> bool {
        let port = match self.ports {
            LivePorts::All => true,
            LivePorts::One(id) => id == source.port,
            LivePorts::Nothing => false,
        };
        port && self.channel.is_none_or(|c| c == source.channel)
    }
}

/// The sending end of the live-note queue, for the thread a MIDI driver calls back on.
pub struct LiveSender {
    producer: Producer<(LiveSource, LiveEvent, u64)>,
}

impl LiveSender {
    /// Queues `event`, from an unknown port on channel 1, for the start of the next block: only
    /// tracks that hear every port play it. Returns false when it was dropped (see
    /// [`LiveSender::send_from`]).
    pub fn send(&mut self, event: LiveEvent) -> bool {
        self.send_from(LiveSource::default(), event)
    }

    /// Queues `event` from `source` for the start of the next block. Returns false when it was
    /// dropped: the queue is full, or the renderer it fed is gone.
    pub fn send_from(&mut self, source: LiveSource, event: LiveEvent) -> bool {
        self.send_at(source, event, 0)
    }

    /// Queues `event` from `source`, played at `time` on the live clock ([`live_now`]; 0 for
    /// no time): it sounds a fixed delay after that time (#269). Returns false when it was
    /// dropped, as [`LiveSender::send_from`] does.
    pub fn send_at(&mut self, source: LiveSource, event: LiveEvent, time: u64) -> bool {
        !self.producer.is_abandoned() && self.producer.push((source, event, time)).is_ok()
    }

    /// The renderer still takes notes from this sender.
    pub fn is_connected(&self) -> bool {
        !self.producer.is_abandoned()
    }
}

impl std::fmt::Debug for LiveSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveSender").field("connected", &self.is_connected()).finish()
    }
}

/// One source's keys and pedal.
#[derive(Clone, Copy)]
struct Keys {
    source: LiveSource,
    /// The keys held down now.
    held: [bool; 128],
    /// The sustain pedal is down.
    sustain: bool,
    /// Keys released while the pedal is down, sounding until it comes up.
    sustained: [bool; 128],
    /// The pitch bend is away from the centre, or the pressure above none: what a track that
    /// stops hearing the source is set back from, so its instrument is not left bent.
    bent: bool,
    pressed: bool,
}

impl Keys {
    const EMPTY: Self = Self {
        source: LiveSource { port: 0, channel: 0 },
        held: [false; 128],
        sustain: false,
        sustained: [false; 128],
        bent: false,
        pressed: false,
    };

    /// `key` sounds because of this source: held down, or sustained by the pedal.
    fn sounding(&self, key: u8) -> bool {
        let at = usize::from(key);
        self.held.get(at).copied().unwrap_or(false) || self.sustained.get(at).copied().unwrap_or(false)
    }

    fn silent(&self) -> bool {
        !self.held.contains(&true) && !self.sustained.contains(&true) && !self.sustain && !self.bent && !self.pressed
    }

    /// Sets `chain`'s pitch bend and pressure back, at `frame` of the block, where this source
    /// moved them (#812).
    fn let_go_controls(&self, chain: &mut Chain, frame: usize) {
        if self.bent {
            chain.live_control(frame, NoteEvent::PitchBend(0.0));
        }
        if self.pressed {
            chain.live_control(frame, NoteEvent::Pressure(0.0));
        }
    }
}

/// Where a stamped control goes ([`Live::place_control`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// In place of one moving the same control, already waiting.
    Merged,
    /// In a place of its own, as a note waits.
    Own,
    /// Nowhere: it plays at once.
    Now,
}

/// `a` and `b` move the same control (one controller, the pitch bend or the pressure), so a
/// later one may take the place of an earlier one still waiting.
fn same_control(a: &LiveEvent, b: &LiveEvent) -> bool {
    match (a, b) {
        (LiveEvent::Controller { number: x, .. }, LiveEvent::Controller { number: y, .. }) => x == y,
        (LiveEvent::PitchBend(_), LiveEvent::PitchBend(_)) | (LiveEvent::Pressure(_), LiveEvent::Pressure(_)) => true,
        _ => false,
    }
}

/// `event` moves a control rather than a key or the pedal.
fn is_control(event: &LiveEvent) -> bool {
    matches!(event, LiveEvent::Controller { .. } | LiveEvent::PitchBend(_) | LiveEvent::Pressure(_))
}

/// A note taken from the queue, waiting for its frame.
#[derive(Clone, Copy)]
struct Waiting {
    /// The renderer frame it plays at.
    at: u64,
    source: LiveSource,
    event: LiveEvent,
}

/// Where the render thread is on the live clock: renderer frame `position` is rendered at
/// `time` (microseconds) on a steady cadence of blocks.
#[derive(Clone, Copy)]
struct Cadence {
    position: u64,
    time: f64,
}

/// The render thread's end: the queue, the armed tracks with their inputs, and each source's
/// keys.
pub(crate) struct Live {
    consumer: Option<Consumer<(LiveSource, LiveEvent, u64)>>,
    armed: [(TrackId, LiveFilter); MAX_ARMED],
    armed_count: usize,
    monitored: [(TrackId, LiveFilter); MAX_ARMED],
    monitored_count: usize,
    keys: [Keys; MAX_SOURCES],
    keys_count: usize,
    /// Stamped notes whose frame is in a later block, in the order they came.
    waiting: [Waiting; MAX_WAITING],
    waiting_count: usize,
    /// The earliest frame a waiting note plays at.
    next_due: u64,
    /// The render thread's place on the live clock, once it has said (`set_clock`).
    cadence: Option<Cadence>,
    /// How long after its time a stamped note plays, in frames.
    delay: u64,
    sample_rate: f64,
    /// Where each note was played (renderer frame), for the tests.
    #[cfg(test)]
    pub(crate) played: Vec<(u64, LiveEvent)>,
}

impl Live {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            consumer: None,
            armed: [(TrackId(0), LiveFilter::default()); MAX_ARMED],
            armed_count: 0,
            monitored: [(TrackId(0), LiveFilter::default()); MAX_ARMED],
            monitored_count: 0,
            keys: [Keys::EMPTY; MAX_SOURCES],
            keys_count: 0,
            waiting: [Waiting { at: 0, source: LiveSource::default(), event: LiveEvent::AllNotesOff }; MAX_WAITING],
            waiting_count: 0,
            next_due: u64::MAX,
            cadence: None,
            delay: 0,
            sample_rate: f64::from(sample_rate.max(1)),
            #[cfg(test)]
            played: Vec::new(),
        }
    }

    /// A new queue to the renderer; the previous sender is cut off.
    pub(crate) fn sender(&mut self) -> LiveSender {
        let (producer, consumer) = RingBuffer::new(LIVE_QUEUE);
        self.consumer = Some(consumer);
        LiveSender { producer }
    }

    /// The render thread is about to render renderer frame `position` at `now` on the live
    /// clock; stamped notes play `delay` frames after their time. The frames are kept on a
    /// steady cadence (each block follows the last by its length) that is pulled slowly
    /// towards `now`, and started again at `now` when the two are further apart than the delay
    /// (the first block, a pause, an underrun): the render thread's own jitter does not move
    /// the notes.
    pub(crate) fn set_clock(&mut self, now: u64, delay: u32, position: u64) {
        self.delay = u64::from(delay);
        let now = now as f64;
        let per_frame = 1e6 / self.sample_rate;
        let steady = self.cadence.map(|c| c.time + position.saturating_sub(c.position) as f64 * per_frame);
        let time = match steady {
            Some(steady) if (now - steady).abs() <= self.delay as f64 * per_frame => {
                steady + (now - steady) * CLOCK_PULL
            }
            _ => now,
        };
        self.cadence = Some(Cadence { position, time });
    }

    /// The renderer frame a note stamped `time` plays at, never before `position`; `position`
    /// for a note without a time or before the render thread has said where it is.
    fn frame_of(&self, time: u64, position: u64) -> u64 {
        let Some(cadence) = self.cadence.filter(|_| time != 0) else { return position };
        let frames = (time as f64 - cadence.time) * self.sample_rate / 1e6;
        let at = cadence.position as f64 + frames + self.delay as f64;
        if at.is_finite() && at > position as f64 { at.round() as u64 } else { position }
    }

    fn armed(&self) -> &[(TrackId, LiveFilter)] {
        self.armed.get(..self.armed_count).unwrap_or_default()
    }

    /// Frames a stamped note plays after it was played.
    pub(crate) fn delay(&self) -> u64 {
        self.delay
    }

    /// Whether `track` is armed.
    pub(crate) fn is_armed(&self, track: TrackId) -> bool {
        self.armed().iter().any(|(t, _)| *t == track)
    }

    /// Whether any track is armed or monitors MIDI: keep the shallower live queue (ADR 0017).
    pub(crate) fn any_armed(&self) -> bool {
        self.armed_count > 0 || self.monitored_count > 0
    }

    /// Arms `track` to hear what `input` takes, or disarms it. Arming an armed track changes
    /// what it hears. A track that is disarmed, or stops hearing a source, lets go of that
    /// source's keys at `frame` of the block; a newly armed one hears the next key pressed, not
    /// the ones already down.
    pub(crate) fn set_armed(
        &mut self,
        chains: &mut [Chain],
        track: TrackId,
        armed: bool,
        input: LiveFilter,
        frame: usize,
    ) {
        self.set_input(chains, track, armed, armed, input, frame);
    }

    fn update_targets(
        targets: &mut [(TrackId, LiveFilter)],
        count: &mut usize,
        track: TrackId,
        enabled: bool,
        input: LiveFilter,
    ) -> Option<LiveFilter> {
        let index = targets.get(..*count).unwrap_or_default().iter().position(|(t, _)| *t == track);
        let before = index.and_then(|i| targets.get(i)).map(|(_, f)| *f);
        match (enabled, index) {
            (true, None) => {
                if let Some(slot) = targets.get_mut(*count) {
                    *slot = (track, input);
                    *count += 1;
                }
            }
            (true, Some(index)) => {
                if let Some(slot) = targets.get_mut(index) {
                    slot.1 = input;
                }
            }
            (false, Some(index)) => {
                let last = count.saturating_sub(1);
                targets.swap(index, last);
                *count = last;
            }
            (false, None) => {}
        }
        before
    }

    /// Recording and monitoring are independent; both target lists are bounded and preallocated.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_input(
        &mut self,
        chains: &mut [Chain],
        track: TrackId,
        armed: bool,
        monitoring: bool,
        input: LiveFilter,
        frame: usize,
    ) {
        Self::update_targets(&mut self.armed, &mut self.armed_count, track, armed, input);
        let before = Self::update_targets(&mut self.monitored, &mut self.monitored_count, track, monitoring, input);
        // Let go of what the track heard and no longer does.
        let Some(before) = before else { return };
        let keys = self.keys.get(..self.keys_count).unwrap_or_default();
        for chain in chains.iter_mut().filter(|c| c.instrument && c.track == track) {
            for source in keys.iter().filter(|k| before.takes(k.source) && !(monitoring && input.takes(k.source))) {
                for key in 0..=127u8 {
                    if source.sounding(key) {
                        chain.release_key(frame, key);
                    }
                }
                source.let_go_controls(chain, frame);
            }
        }
    }

    /// The keys of `source`, a slot found or taken for it.
    fn keys_of(&mut self, source: LiveSource) -> Option<usize> {
        let count = self.keys_count;
        if let Some(found) = self.keys.get(..count).unwrap_or_default().iter().position(|k| k.source == source) {
            return Some(found);
        }
        let slot = if count < MAX_SOURCES {
            self.keys_count += 1;
            count
        } else {
            // Every slot is taken: reuse one with nothing sounding, else the last.
            self.keys.iter().position(Keys::silent).unwrap_or(MAX_SOURCES - 1)
        };
        let keys = self.keys.get_mut(slot)?;
        *keys = Keys { source, ..Keys::EMPTY };
        Some(slot)
    }

    /// Takes what the controllers sent since the last block, for a block starting at renderer
    /// frame `position`: a note without a time, or one whose frame has passed, plays at the
    /// block's first frame; a stamped one waits for its frame ([`Live::frame`]). Call it once
    /// the chains have begun the block.
    pub(crate) fn receive(&mut self, chains: &mut [Chain], recorder: &mut Recorder, position: u64) {
        // Bounded: what arrives while this runs waits for the next block.
        for _ in 0..LIVE_QUEUE {
            let Some(Ok((source, event, time))) = self.consumer.as_mut().map(Consumer::pop) else { break };
            let at = self.frame_of(time, position);
            // A note never overtakes one that came before it from the same source.
            let after = self
                .waiting
                .get(..self.waiting_count)
                .unwrap_or_default()
                .iter()
                .filter(|w| w.source == source)
                .map(|w| w.at)
                .max()
                .unwrap_or(0);
            let at = at.max(after);
            let place =
                if at > position && is_control(&event) { self.place_control(source, event) } else { Place::Own };
            if place == Place::Merged {
                continue;
            }
            if at > position
                && place == Place::Own
                && let Some(slot) = self.waiting.get_mut(self.waiting_count)
            {
                *slot = Waiting { at, source, event };
                self.waiting_count += 1;
                self.next_due = self.next_due.min(at);
                continue;
            }
            self.play(chains, recorder, source, event, 0, position);
        }
        self.frame(chains, recorder, 0, position);
    }

    /// Where a stamped control from `source` waits: in a place of its own, as a note does, while
    /// controls have fewer than [`MAX_WAITING_CONTROLS`], so every value plays at its own frame
    /// and a wheel moves smoothly. Once they have that many, the latest one waiting from the
    /// source that moves the same control takes its value, at that one's frame (never a later
    /// one, which a stream would keep pushing back); with none, it plays at once (a control has
    /// no key to overtake). Never allocates.
    fn place_control(&mut self, source: LiveSource, event: LiveEvent) -> Place {
        let waiting = self.waiting.get(..self.waiting_count).unwrap_or_default();
        if waiting.iter().filter(|w| is_control(&w.event)).count() < MAX_WAITING_CONTROLS {
            return Place::Own;
        }
        let same = waiting.iter().rposition(|w| w.source == source && same_control(&w.event, &event));
        let Some(slot) = same.and_then(|index| self.waiting.get_mut(index)) else { return Place::Now };
        slot.event = event;
        Place::Merged
    }

    /// Plays the waiting notes due by renderer frame `position`, at `frame` of the block, in the
    /// order they came. Never allocates.
    pub(crate) fn frame(&mut self, chains: &mut [Chain], recorder: &mut Recorder, frame: usize, position: u64) {
        if self.next_due > position {
            return;
        }
        let mut kept = 0;
        let mut next_due = u64::MAX;
        for index in 0..self.waiting_count {
            let Some(waiting) = self.waiting.get(index).copied() else { break };
            if waiting.at <= position {
                self.play(chains, recorder, waiting.source, waiting.event, frame, position);
            } else {
                if let Some(slot) = self.waiting.get_mut(kept) {
                    *slot = waiting;
                }
                kept += 1;
                next_due = next_due.min(waiting.at);
            }
        }
        self.waiting_count = kept;
        self.next_due = next_due;
    }

    /// Plays one note from `source` at `frame` of the block (renderer frame `position`) on the
    /// instrument chains of the armed tracks that hear the source, and records it on each of
    /// those tracks while recording is on (a key down, or a sound let go).
    fn play(
        &mut self,
        chains: &mut [Chain],
        recorder: &mut Recorder,
        source: LiveSource,
        event: LiveEvent,
        frame: usize,
        position: u64,
    ) {
        #[cfg(test)]
        self.played.push((position, event));
        let Some(slot) = self.keys_of(source) else { return };
        let Self { armed, armed_count, monitored, monitored_count, keys, .. } = self;
        let armed = armed.get(..*armed_count).unwrap_or_default();
        let monitored = monitored.get(..*monitored_count).unwrap_or_default();
        let Some(keys) = keys.get_mut(slot) else { return };
        let hearing = || armed.iter().filter(|(_, f)| f.takes(source)).map(|(t, _)| *t);
        let targets = chains
            .iter_mut()
            .filter(|c| c.instrument && monitored.iter().any(|(t, f)| *t == c.track && f.takes(source)));
        match event {
            LiveEvent::NoteOn { key, velocity } => {
                let Some(held) = keys.held.get_mut(usize::from(key)) else { return };
                *held = true;
                if let Some(sustained) = keys.sustained.get_mut(usize::from(key)) {
                    *sustained = false;
                }
                let velocity = if velocity.is_finite() { velocity } else { 1.0 };
                for chain in targets {
                    chain.note_on(frame, position, key, velocity, UNTIL_RELEASED);
                }
                for track in hearing() {
                    recorder.note(track, position, key, Some(velocity));
                }
            }
            LiveEvent::NoteOff { key } => {
                let Some(held) = keys.held.get_mut(usize::from(key)) else { return };
                if !*held {
                    return;
                }
                *held = false;
                if keys.sustain {
                    if let Some(sustained) = keys.sustained.get_mut(usize::from(key)) {
                        *sustained = true;
                    }
                    return;
                }
                for chain in targets {
                    chain.release_key(frame, key);
                }
                for track in hearing() {
                    recorder.note(track, position, key, None);
                }
            }
            LiveEvent::Controller { number, value } => {
                let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { return };
                for chain in targets {
                    chain.live_control(frame, NoteEvent::Controller { number, value });
                }
            }
            LiveEvent::PitchBend(value) => {
                let value = if value.is_finite() { value.clamp(-1.0, 1.0) } else { return };
                keys.bent = value != 0.0;
                for chain in targets {
                    chain.live_control(frame, NoteEvent::PitchBend(value));
                }
            }
            LiveEvent::Pressure(value) => {
                let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { return };
                keys.pressed = value > 0.0;
                for chain in targets {
                    chain.live_control(frame, NoteEvent::Pressure(value));
                }
            }
            LiveEvent::Sustain { on } => {
                keys.sustain = on;
                if on {
                    return;
                }
                for chain in targets {
                    for key in 0..=127u8 {
                        if keys.sustained.get(usize::from(key)).copied().unwrap_or(false) {
                            chain.release_key(frame, key);
                        }
                    }
                }
                for key in 0..=127u8 {
                    if keys.sustained.get(usize::from(key)).copied().unwrap_or(false) {
                        for track in hearing() {
                            recorder.note(track, position, key, None);
                        }
                    }
                }
                keys.sustained = [false; 128];
            }
            LiveEvent::AllNotesOff => {
                for chain in targets {
                    for key in 0..=127u8 {
                        if keys.sounding(key) {
                            chain.release_key(frame, key);
                        }
                    }
                    // A closed port leaves no wheel bent (#812).
                    keys.let_go_controls(chain, frame);
                }
                for key in 0..=127u8 {
                    if keys.sounding(key) {
                        for track in hearing() {
                            recorder.note(track, position, key, None);
                        }
                    }
                }
                *keys = Keys { source, ..Keys::EMPTY };
            }
        }
    }
}
