// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded per-port MIDI 1.0 to native-note translation (MPE specification 1.1).
use suhina_device_api::{
    NoteExpressionKind as Kind, NoteExpressionState, NoteIdAllocator, NoteInstanceEvent as Event, NoteInstanceId,
};
use suhina_engine::LiveEvent;
use suhina_session::{MpeConfig, MpeMode};

/// Protocol occurrences retained per physical input, including released tails. With no
/// device completion return path, only the oldest released occurrence may be reclaimed,
/// by an explicit choke before its slot is reused. Held/pedal-held notes are never stolen.
pub const MAX_OCCURRENCES: usize = 256;

#[derive(Clone, Copy)]
struct Channel {
    bend: f64,
    pressure: f64,
    timbre: f64,
    pedal: bool,
    rpn_msb: Option<u8>,
    rpn_lsb: Option<u8>,
}

impl Channel {
    const EMPTY: Self = Self { bend: 0., pressure: 0., timbre: 0.5, pedal: false, rpn_msb: None, rpn_lsb: None };
}

#[derive(Clone, Copy)]
struct Occurrence {
    id: NoteInstanceId,
    channel: u8,
    key: u8,
    held: bool,
    sustained: bool,
    release_velocity: f32,
    /// Member pitch captured at key-up; manager pitch can continue moving the tail.
    member_pitch: f64,
    expression: NoteExpressionState,
}

/// One input's protocol state. Construction/configuration is off the audio thread;
/// message handling itself has fixed memory/work limits and performs no allocation.
pub struct MpeInput {
    config: MpeConfig,
    channels: [Channel; 16],
    notes: [Option<Occurrence>; MAX_OCCURRENCES],
    ids: NoteIdAllocator,
}

type Sink<'a> = dyn FnMut(u8, LiveEvent) -> bool + 'a;

impl MpeInput {
    /// `origin` must be unique to this port activation, including across reconnects.
    pub fn new(config: MpeConfig, origin: u64) -> Option<Self> {
        if !config.is_valid() {
            return None;
        }
        let mut config = config;
        if config.mode != MpeMode::Manual {
            config.lower_members = 0;
            config.upper_members = 0;
        }
        Some(Self {
            config,
            channels: [Channel::EMPTY; 16],
            notes: [None; MAX_OCCURRENCES],
            ids: NoteIdAllocator::new(origin)?,
        })
    }

    /// Active configuration (automatic counts/ranges may differ from the saved defaults).
    pub fn config(&self) -> MpeConfig {
        self.config
    }

    fn zone(&self, channel: u8) -> Option<usize> {
        if self.config.mode == MpeMode::Off {
            return None;
        }
        if self.config.lower_members > 0 && channel <= self.config.lower_members {
            Some(0)
        } else if self.config.upper_members > 0 && channel >= 15 - self.config.upper_members {
            Some(1)
        } else {
            None
        }
    }

    fn manager(zone: usize) -> u8 {
        if zone == 0 { 0 } else { 15 }
    }

    fn range(&self, channel: u8) -> f64 {
        match self.zone(channel) {
            Some(0) if channel == 0 => self.config.lower_manager_bend,
            Some(0) => self.config.lower_member_bend,
            Some(_) if channel == 15 => self.config.upper_manager_bend,
            Some(_) => self.config.upper_member_bend,
            None => 2,
        }
        .into()
    }

    fn state(&self, channel: u8) -> NoteExpressionState {
        let Some(member) = self.channels.get(usize::from(channel)) else {
            return NoteExpressionState::NEUTRAL;
        };
        let Some(zone) = self.zone(channel) else {
            return NoteExpressionState::NEUTRAL;
        };
        let manager_channel = Self::manager(zone);
        let manager = self.channels[usize::from(manager_channel)];
        let own = channel == manager_channel;
        NoteExpressionState::new(
            member.bend * self.range(channel) + if own { 0. } else { manager.bend * self.range(manager_channel) },
            if own { member.pressure } else { member.pressure.max(manager.pressure) },
            if own { member.timbre } else { (member.timbre + manager.timbre - 0.5).clamp(0., 1.) },
        )
        .unwrap_or(NoteExpressionState::NEUTRAL)
    }

    fn emit(note: Occurrence, event: Event, sink: &mut Sink<'_>) -> bool {
        sink(note.channel, LiveEvent::Instance { id: note.id, event })
    }

    /// Immediately terminates every retained occurrence on this physical port.
    pub fn panic(&mut self, sink: &mut Sink<'_>) {
        for slot in &mut self.notes {
            if let Some(note) = slot.take() {
                Self::emit(note, Event::Choke { key: note.key }, sink);
            }
        }
        self.channels.fill(Channel::EMPTY);
    }

    /// Reads one complete channel message; malformed/unsupported messages do nothing.
    /// A false sink response means a downstream overflow, for which the engine's queue has
    /// an out-of-band panic flag. The producer does not invent a successfully started note.
    pub fn process(&mut self, message: &[u8], sink: &mut Sink<'_>) {
        // Off is exactly the established channel/key path, including mono/legato
        // allocation. Identified ordinary channels and poly key pressure are opt-in.
        if self.config.mode == MpeMode::Off {
            if let Some((channel, event)) = crate::parse(message) {
                sink(channel, event);
            }
            return;
        }
        let Some((&status, data)) = message.split_first() else {
            return;
        };
        let channel = status & 15;
        let family = status & 0xf0;
        let needed = if family == 0xd0 { 1 } else { 2 };
        let Some(data) = data.get(..needed) else {
            return;
        };
        if !(0x80..=0xe0).contains(&family) || data.iter().any(|b| *b > 127) {
            return;
        }
        let first = data[0];
        let second = data.get(1).copied().unwrap_or(0);
        match family {
            0x90 if second > 0 => self.on(channel, first, f32::from(second) / 127., sink),
            0x80 | 0x90 => self.off(channel, first, f32::from(second) / 127., sink),
            0xa0 => self.poly_pressure(channel, first, f64::from(second) / 127., sink),
            0xb0 => self.control(channel, first, second, sink),
            0xd0 => {
                self.channels[usize::from(channel)].pressure = f64::from(first) / 127.;
                if self.zone(channel).is_some() {
                    self.express(channel, Kind::Pressure, sink);
                } else {
                    sink(channel, LiveEvent::Pressure(f32::from(first) / 127.));
                }
            }
            0xe0 => {
                let bend = f64::from(u16::from(first) | (u16::from(second) << 7));
                let bend = (bend - 8192.) / 8192.;
                self.channels[usize::from(channel)].bend = bend;
                if self.zone(channel).is_some() {
                    self.express(channel, Kind::Pitch, sink);
                } else {
                    sink(channel, LiveEvent::PitchBend(bend as f32));
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, channel: u8, key: u8, velocity: f32, sink: &mut Sink<'_>) {
        let index = self.notes.iter().position(Option::is_none).or_else(|| {
            self.notes
                .iter()
                .enumerate()
                .filter_map(|(i, n)| n.filter(|n| !n.held && !n.sustained).map(|n| (i, n.id.serial())))
                .min_by_key(|(_, age)| *age)
                .map(|(i, _)| i)
        });
        let Some(index) = index else {
            return;
        };
        let Some(id) = self.ids.allocate() else {
            return;
        };
        if let Some(old) = self.notes[index].take() {
            Self::emit(old, Event::Choke { key: old.key }, sink);
        }
        let note = Occurrence {
            id,
            channel,
            key,
            held: true,
            sustained: false,
            release_velocity: 0.,
            member_pitch: 0.,
            expression: self.state(channel),
        };
        if Self::emit(note, Event::On { key, velocity, expression: note.expression }, sink) {
            self.notes[index] = Some(note);
        }
    }

    fn off(&mut self, channel: u8, key: u8, velocity: f32, sink: &mut Sink<'_>) {
        let index = self
            .notes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                n.filter(|n| n.held && n.channel == channel && n.key == key).map(|n| (i, n.id.serial()))
            })
            .min_by_key(|(_, age)| *age)
            .map(|(i, _)| i);
        let Some(index) = index else {
            return;
        };
        self.release(index, velocity, sink);
    }

    fn release(&mut self, index: usize, velocity: f32, sink: &mut Sink<'_>) {
        let Some(mut note) = self.notes.get(index).copied().flatten() else {
            return;
        };
        if !note.held {
            return;
        }
        let manager = self.zone(note.channel).map(Self::manager).unwrap_or(note.channel);
        note.held = false;
        note.sustained = self.channels[usize::from(manager)].pedal;
        note.release_velocity = velocity;
        note.member_pitch = self.channels[usize::from(note.channel)].bend * self.range(note.channel);
        if !note.sustained {
            Self::emit(note, Event::Off { key: note.key, velocity }, sink);
        }
        self.notes[index] = Some(note);
        // Detach the occurrence, not the channel latch: MPE 1.1 §§2.2.6–8 require
        // the most recently received controls to initialize the next note too.
    }

    fn scope(&self, sender: u8, note: u8) -> bool {
        match self.zone(sender) {
            Some(zone) if sender == Self::manager(zone) => self.zone(note) == Some(zone),
            _ => sender == note,
        }
    }

    fn express(&mut self, sender: u8, kind: Kind, sink: &mut Sink<'_>) {
        for index in 0..MAX_OCCURRENCES {
            let Some(mut note) = self.notes[index] else {
                continue;
            };
            if !self.scope(sender, note.channel) {
                continue;
            }
            let manager = self.zone(note.channel).map(Self::manager);
            let value = if note.held {
                self.state(note.channel).value(kind)
            } else if kind == Kind::Pitch && manager == Some(sender) {
                if note.channel == sender {
                    self.channels[usize::from(sender)].bend * self.range(sender)
                } else {
                    note.member_pitch + self.channels[usize::from(sender)].bend * self.range(sender)
                }
            } else {
                continue;
            };
            note.expression.set(kind, value);
            Self::emit(note, Event::Expression { kind, value }, sink);
            self.notes[index] = Some(note);
        }
    }

    fn poly_pressure(&mut self, channel: u8, key: u8, value: f64, sink: &mut Sink<'_>) {
        if self.zone(channel).is_some_and(|zone| Self::manager(zone) != channel) {
            return;
        }
        for note in self.notes.iter_mut().flatten().filter(|n| n.channel == channel && n.key == key && n.held) {
            note.expression.set(Kind::Pressure, value);
            Self::emit(*note, Event::Expression { kind: Kind::Pressure, value }, sink);
        }
    }

    fn pedal(&mut self, channel: u8, down: bool, sink: &mut Sink<'_>) {
        if self.zone(channel).is_some_and(|zone| Self::manager(zone) != channel) {
            return;
        }
        self.channels[usize::from(channel)].pedal = down;
        if down {
            return;
        }
        for index in 0..MAX_OCCURRENCES {
            let Some(mut note) = self.notes[index] else {
                continue;
            };
            if note.sustained && self.scope(channel, note.channel) {
                note.sustained = false;
                Self::emit(note, Event::Off { key: note.key, velocity: note.release_velocity }, sink);
                self.notes[index] = Some(note);
            }
        }
    }

    fn control(&mut self, channel: u8, number: u8, value: u8, sink: &mut Sink<'_>) {
        let state = &mut self.channels[usize::from(channel)];
        match number {
            101 => state.rpn_msb = Some(value),
            100 => state.rpn_lsb = Some(value),
            98 | 99 => {
                state.rpn_msb = None;
                state.rpn_lsb = None;
            }
            6 => {
                let selection = state.rpn_msb.zip(state.rpn_lsb);
                match selection {
                    Some((0, 6))
                        if self.config.mode == MpeMode::Automatic && matches!(channel, 0 | 15) && value <= 15 =>
                    {
                        self.configure_zone(channel, value, sink)
                    }
                    Some((0, 0)) if self.config.mode == MpeMode::Automatic && value <= 96 => {
                        self.configure_range(channel, value, sink)
                    }
                    _ => {}
                }
            }
            64 => self.pedal(channel, value >= 64, sink),
            74 if self.zone(channel).is_some() => {
                self.channels[usize::from(channel)].timbre =
                    if value <= 64 { f64::from(value) / 128. } else { 0.5 + f64::from(value - 64) / 126. };
                self.express(channel, Kind::Timbre, sink);
            }
            120 | 123 => {
                for index in 0..MAX_OCCURRENCES {
                    let Some(note) = self.notes[index] else {
                        continue;
                    };
                    if self.scope(channel, note.channel) {
                        if number == 120 {
                            Self::emit(note, Event::Choke { key: note.key }, sink);
                            self.notes[index] = None;
                        } else {
                            self.release(index, 0., sink);
                        }
                    }
                }
                if number == 120 {
                    self.channels[usize::from(channel)].pedal = false;
                }
            }
            121 => {
                for affected in 0..16u8 {
                    if self.scope(channel, affected) {
                        self.channels[usize::from(affected)] = Channel::EMPTY;
                    }
                }
                self.pedal(channel, false, sink);
                for kind in [Kind::Pitch, Kind::Pressure, Kind::Timbre] {
                    self.express(channel, kind, sink);
                }
                if self.zone(channel).is_none() {
                    sink(channel, LiveEvent::PitchBend(0.));
                    sink(channel, LiveEvent::Pressure(0.));
                }
            }
            _ => {
                sink(channel, LiveEvent::Controller { number, value: f32::from(value) / 127. });
            }
        }
    }

    fn configure_range(&mut self, channel: u8, range: u8, sink: &mut Sink<'_>) {
        let Some(zone) = self.zone(channel) else {
            return;
        };
        match (zone, channel == Self::manager(zone)) {
            (0, true) => self.config.lower_manager_bend = range,
            (0, false) => self.config.lower_member_bend = range,
            (_, true) => self.config.upper_manager_bend = range,
            (_, false) => self.config.upper_member_bend = range,
        }
        if channel == Self::manager(zone) {
            self.express(channel, Kind::Pitch, sink);
        } else {
            for member in 0..16u8 {
                if self.zone(member) == Some(zone) && member != Self::manager(zone) {
                    self.express(member, Kind::Pitch, sink);
                }
            }
        }
    }

    fn configure_zone(&mut self, manager: u8, count: u8, sink: &mut Sink<'_>) {
        let before: [Option<usize>; 16] = std::array::from_fn(|c| self.zone(c as u8));
        if manager == 0 {
            self.config.lower_members = count;
            if count > 0 {
                self.config.upper_members = self.config.upper_members.min(14u8.saturating_sub(count));
            }
        } else {
            self.config.upper_members = count;
            if count > 0 {
                self.config.lower_members = self.config.lower_members.min(14u8.saturating_sub(count));
            }
        }
        let changed: [bool; 16] = std::array::from_fn(|c| before[c] != self.zone(c as u8));
        for slot in &mut self.notes {
            if let Some(note) = slot.filter(|n| changed[usize::from(n.channel)]) {
                Self::emit(note, Event::Choke { key: note.key }, sink);
                *slot = None;
            }
        }
        for (channel, changed) in changed.into_iter().enumerate() {
            if changed {
                self.channels[channel] = Channel::EMPTY;
            }
        }
    }
}

#[cfg(test)]
mod tests;
