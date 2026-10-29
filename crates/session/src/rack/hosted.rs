// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio half of a hosted plug-in, as the engine runs it on the audio thread: its events
//! put in order, its block sent and its answer read within the deadline.

use super::*;

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
    gate: Option<super::transport_gate::TransportGate>,
    continuation: suhina_engine::SongPosition,
    rate: u32,
    /// Live notes arriving during the stop fade wait for the first stopped block, so
    /// its transport transition cannot reset a newly started voice.
    stop_notes: Vec<DeviceEvent>,
    stop_notes_overflow: bool,
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
    /// Where the song is, as the plug-in is told with the next block: its tempo-synced parts
    /// and an ARA renderer (ADR 0028) follow it.
    song: suhina_engine::SongPosition,
}

impl Hosted {
    pub fn new(audio: PluginAudio) -> Self {
        Self {
            gate: None,
            continuation: suhina_engine::SongPosition::default(),
            rate: 1,
            stop_notes: Vec::with_capacity(MAX_EVENTS / 2),
            stop_notes_overflow: false,
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
            song: suhina_engine::SongPosition::default(),
        }
    }

    /// The counts this keeps while the engine runs it.
    pub fn counts(&self) -> Arc<BlockCounts> {
        Arc::clone(&self.counts)
    }

    /// Instruments fade before a transport stop; downstream effects keep their tails.
    pub fn with_instrument_transport(mut self, instrument: bool, rate: u32) -> Self {
        self.gate = instrument.then(|| super::transport_gate::TransportGate::new(rate));
        self.rate = rate.max(1);
        self
    }
}

/// The events of the song's place sent with every block (transport, tempo, beat, bar, the loop
/// in beats and in seconds): room for them is kept in a block's parameter events.
pub(super) const SONG_EVENTS: usize = 6;

/// Moves what earlier blocks could not send from `carried` to the start of this block, at its
/// first frame: parameter values into `params` while `reserve` places are left for the song's
/// place, the rest into `notes` while it has room. What does not fit stays carried for the next
/// block. Never allocates.
pub(super) fn take_carried(carried: &mut Vec<Event>, params: &mut Vec<Event>, notes: &mut Vec<Event>, reserve: usize) {
    let params_room = params.capacity().saturating_sub(reserve);
    let notes_room = notes.capacity();
    carried.retain(|event| {
        let event = Event { frame: 0, ..*event };
        let (to, room) = match event.kind {
            EventKind::Param { .. } => (&mut *params, params_room),
            _ => (&mut *notes, notes_room),
        };
        if to.len() < room {
            to.push(event);
            false
        } else {
            true
        }
    });
}

/// Keeps the releases among `events` in `carried`, and the latest value of each parameter and
/// of each channel's pitch bend and pressure, within its capacity: a wheel let go in a block
/// that was not sent still comes back to rest. Never allocates.
pub(super) fn carry_releases(events: &[Event], carried: &mut Vec<Event>) {
    let room = carried.capacity();
    let latest = |e: &Event| {
        matches!(e.kind, EventKind::Param { .. } | EventKind::PitchBend { .. } | EventKind::Pressure { .. })
    };
    for event in events.iter().filter(|e| releases(e) || latest(e)) {
        let same = |c: &Event| match (c.kind, event.kind) {
            (EventKind::Param { id: a, .. }, EventKind::Param { id: b, .. }) => a == b,
            (EventKind::PitchBend { channel: a, .. }, EventKind::PitchBend { channel: b, .. }) => a == b,
            (EventKind::Pressure { channel: a, .. }, EventKind::Pressure { channel: b, .. }) => a == b,
            _ => false,
        };
        if let Some(older) = carried.iter_mut().find(|c| same(c)) {
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
pub(super) fn merge(params: &[Event], notes: &[Event], out: &mut Vec<Event>) {
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
pub(super) fn releases(event: &Event) -> bool {
    match event.kind {
        EventKind::NoteOff { .. } => true,
        EventKind::Controller { number, .. } => number == SUSTAIN || number == ALL_NOTES_OFF,
        _ => false,
    }
}

/// `event` at `frame` as the plug-in host takes it, into `out` (within its capacity).
pub(super) fn push_event(out: &mut Vec<Event>, frame: u32, event: DeviceEvent) {
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
        DeviceEvent::Pressure(value) => push(EventKind::Pressure { channel: 0, value: value.clamp(0.0, 1.0) }),
        DeviceEvent::AllNotesOff => {
            push(EventKind::Controller { channel: 0, number: SUSTAIN, value: 0.0 });
            push(EventKind::Controller { channel: 0, number: ALL_NOTES_OFF, value: 0.0 });
        }
        // A plug-in has no use for the scale the notes are in.
        DeviceEvent::On { .. } | DeviceEvent::Off { .. } | DeviceEvent::Controller { .. } | DeviceEvent::Scale(_) => {}
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
        take_carried(&mut self.carried, &mut self.params, &mut self.notes, SONG_EVENTS);
        let holding_stop = self.gate.as_ref().is_some_and(|g| g.holds_transport());
        if !holding_stop {
            for event in self.stop_notes.drain(..) {
                if matches!(event, DeviceEvent::On { .. })
                    && let Some(gate) = &mut self.gate
                {
                    gate.live_note(0);
                }
                push_event(&mut self.notes, 0, event);
            }
            if self.stop_notes_overflow {
                push_event(&mut self.notes, 0, DeviceEvent::AllNotesOff);
                self.stop_notes_overflow = false;
            }
        }
        for &(frame, event) in events {
            if holding_stop && (!self.stop_notes.is_empty() || matches!(event, DeviceEvent::On { .. })) {
                // Reserve room for a final release if hostile input exhausts the bound.
                if self.stop_notes.len() + 1 < self.stop_notes.capacity() {
                    self.stop_notes.push(event);
                } else {
                    self.stop_notes_overflow = true;
                }
                continue;
            }
            if matches!(event, DeviceEvent::On { .. })
                && let Some(gate) = &mut self.gate
            {
                gate.live_note(frame);
            }
            push_event(&mut self.notes, u32::try_from(frame).unwrap_or(u32::MAX), event);
        }
        // Where the song is, with every block: tempo-synced effects and instruments lock to it,
        // an ARA renderer plays by it.
        // During the short stop fade the instrument still renders a continuing transport.
        // In particular, Stop's jump back to the start marker must wait with the stop flag.
        let song = if self.gate.as_ref().is_some_and(|g| g.holds_transport()) { self.continuation } else { self.song };
        self.continuation = song;
        self.continuation.position = song.position.saturating_add((buffer.len() / 2) as u64);
        self.continuation.beat += (buffer.len() / 2) as f64 * song.tempo / (60.0 * f64::from(self.rate));
        let tempo = if song.tempo.is_finite() { song.tempo.clamp(1.0, 999.0) as f32 } else { 120.0 };
        self.params.push(Event::transport(
            song.playing || self.gate.as_ref().is_some_and(|g| g.holds_transport()),
            song.position,
        ));
        self.params.push(Event::tempo(tempo));
        self.params.push(Event::beat(song.beat));
        self.params.push(Event::bar(song.bar, song.bar_start, song.numerator, song.denominator));
        let (start, end) = song.cycle.unwrap_or((0.0, 0.0));
        self.params.push(Event::cycle(song.cycle.is_some(), start as f32, end as f32));
        let (start, end) = song.cycle_seconds.unwrap_or((0.0, 0.0));
        self.params.push(Event::cycle_seconds(start as f32, end as f32));
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
                if let Some(gate) = &mut self.gate {
                    gate.process(buffer);
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
    fn smooth_transport_stop(&self) -> bool {
        self.gate.is_some()
    }
    fn reconfigure(&mut self, sample_rate: u32, timeout: Duration) -> Result<u32, String> {
        let latency = self.audio.reconfigure(f64::from(sample_rate), timeout).map_err(|e| e.to_string())?;
        self.events.clear();
        self.notes.clear();
        self.params.clear();
        self.carried.clear();
        self.stop_notes.clear();
        self.stop_notes_overflow = false;
        self.sent_at = None;
        self.rate = sample_rate.max(1);
        if self.gate.is_some() {
            self.gate = Some(super::transport_gate::TransportGate::new(sample_rate));
        }
        Ok(latency)
    }
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
        if !self.carried.is_empty() || self.gate.as_ref().is_some_and(|g| !g.unity()) {
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

    fn transport(&mut self, song: &suhina_engine::SongPosition) {
        if let Some(gate) = &mut self.gate {
            gate.transport(song.playing);
        }
        self.song = *song;
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
