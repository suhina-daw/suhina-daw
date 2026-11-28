// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Channel-MPE output: a plug-in instrument playing notes that carry expression curves
//! ([`suhina_model::NoteExpression`]) gets them as MPE, Lower Zone, over MIDI 1.0
//! (`docs/spec/mpe.md` › Plug-in boundaries, ADR 0039 › 8).
//!
//! - **When.** A chain whose first slot is a plug-in instrument plays as MPE as soon as a note
//!   it plays (arrangement or launched clip) has a curve; every note of that chain then gets a
//!   member channel. Any other chain plays as before, every note on the first channel.
//! - **Setup.** Before its first note, and again after a stop: the MPE Configuration Message
//!   (RPN 6 on the manager channel, 15 member channels), then on each member channel RPN 0 at
//!   [`BEND_RANGE`] semitones and the neutral controls (bend centred, pressure 0, CC74 64).
//! - **Notes.** A note takes a free member channel, the one released longest ago first; with
//!   all 15 sounding, the oldest note is released and its channel taken (voice stealing). Before
//!   its note-on the channel gets the curves' values where the note starts, each only when it
//!   differs from what the channel last had; while it sounds, each change of a curve's MIDI value
//!   goes out at its frame. Its note-off goes on the same channel.
//! - **Curves** are prepared with the graph ([`Curves`]): each curve sampled at its points and
//!   every [`CURVE_STEP`] beats between them, kept only where the MIDI value changes, in beats
//!   from the note's start. The audio thread reads them and turns beats into frames.

use std::collections::HashMap;

use suhina_device_api::{ChannelMessage, NoteEvent};
use suhina_model::{ExpressionKind, NoteExpression, Shared};

use super::{Chain, DeviceSlot, NOT_HELD};
use suhina_model::TempoMap;

/// Member channels of the Lower Zone: MIDI channels 2 to 16 (1 to 15 counted from 0).
pub(crate) const MEMBERS: usize = 15;
/// The member channels' pitch bend range in semitones, MPE's default.
pub(crate) const BEND_RANGE: u8 = 48;
/// How finely a curve is sampled between its points, in beats.
pub(crate) const CURVE_STEP: f64 = 1.0 / 256.0;
/// Most prepared curve changes one graph holds; notes past it play without their curves.
pub(crate) const MAX_CURVE_CHANGES: usize = 1 << 20;
/// MPE's timbre controller.
const TIMBRE_CC: u8 = 74;
const ALL_NOTES_OFF: u8 = 123;

/// What a curve change moves, as MIDI 1.0 carries it on a member channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dim {
    /// 14-bit pitch bend, 0 to 16383, 8192 centred.
    Bend,
    /// 7-bit channel pressure.
    Pressure,
    /// 7-bit CC74.
    Timbre,
}

impl Dim {
    const ALL: [Dim; 3] = [Dim::Bend, Dim::Pressure, Dim::Timbre];

    fn index(self) -> usize {
        match self {
            Dim::Bend => 0,
            Dim::Pressure => 1,
            Dim::Timbre => 2,
        }
    }

    fn kind(self) -> ExpressionKind {
        match self {
            Dim::Bend => ExpressionKind::Pitch,
            Dim::Pressure => ExpressionKind::Pressure,
            Dim::Timbre => ExpressionKind::Timbre,
        }
    }

    /// The MIDI value of `value` in the kind's units.
    pub(crate) fn midi(self, value: f64) -> u16 {
        match self {
            Dim::Bend => bend_value(value),
            Dim::Pressure => pressure_value(value),
            Dim::Timbre => timbre_value(value),
        }
    }

    fn neutral(self) -> u16 {
        self.midi(self.kind().neutral())
    }

    /// The event that sets this to `value` on `channel`, in the units the plug-in host turns back
    /// into exactly `value`.
    fn event(self, channel: u8, value: u16) -> NoteEvent {
        let message = match self {
            Dim::Bend => ChannelMessage::PitchBend(bend_event(value)),
            Dim::Pressure => ChannelMessage::Pressure(seven_bit(value)),
            Dim::Timbre => ChannelMessage::Controller { number: TIMBRE_CC, value: seven_bit(value) },
        };
        NoteEvent::Channel { channel, message }
    }
}

/// The 14-bit pitch bend of `semitones` at [`BEND_RANGE`]: 8192 + semitones / 48 × 8191,
/// rounded and clamped, so ±48 is 16383 and 1. A bend past the range stays at its end.
pub(crate) fn bend_value(semitones: f64) -> u16 {
    if !semitones.is_finite() {
        return 8192;
    }
    (8192.0 + (semitones / f64::from(BEND_RANGE) * 8191.0).round()).clamp(0.0, 16_383.0) as u16
}

/// Pressure 0 to 1 as 0 to 127.
pub(crate) fn pressure_value(pressure: f64) -> u16 {
    if !pressure.is_finite() {
        return 0;
    }
    (pressure.clamp(0.0, 1.0) * 127.0).round() as u16
}

/// Timbre 0 to 1 as CC74: 0.5 is 64, 0 and 1 are 0 and 127, linear on either side (the mapping
/// MPE input uses, `docs/spec/mpe.md` › Input and zones).
pub(crate) fn timbre_value(timbre: f64) -> u16 {
    if !timbre.is_finite() {
        return 64;
    }
    let timbre = timbre.clamp(0.0, 1.0);
    if timbre <= 0.5 { (timbre * 128.0).round() as u16 } else { 64 + ((timbre - 0.5) * 126.0).round() as u16 }
}

/// The [`NoteEvent`] pitch bend (-1 to 1) the plug-in host turns into the 14-bit `value`: it
/// sends `round((bend + 1) / 2 × 16383)`.
pub(crate) fn bend_event(value: u16) -> f32 {
    (f64::from(value.min(16_383)) * 2.0 / 16_383.0 - 1.0) as f32
}

/// A 7-bit `value` as the 0-to-1 value the plug-in host turns back into it (`round(v × 127)`).
fn seven_bit(value: u16) -> f32 {
    f32::from(value.min(127)) / 127.0
}

/// One change of a note's curve: at `at` beats from the note's start, `dim` becomes `value`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CurveChange {
    pub(crate) at: f32,
    pub(crate) dim: Dim,
    pub(crate) value: u16,
}

/// A note's changes in its list: `first..end`. Empty for a note without curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CurveRange {
    pub(crate) first: u32,
    pub(crate) end: u32,
}

impl CurveRange {
    pub(crate) fn is_empty(self) -> bool {
        self.end <= self.first
    }
}

/// The prepared curves of a graph's notes, or of a launch clip's: built off the audio thread,
/// read on it. A note played many times (a loop's passes) is prepared once.
#[derive(Default)]
pub(crate) struct Curves {
    pub(crate) changes: Vec<CurveChange>,
    prepared: HashMap<(usize, u64), CurveRange>,
}

impl Curves {
    /// Prepares `expression` for a note played `length` beats; an empty range for a note without
    /// curves, and for one that would take the list past [`MAX_CURVE_CHANGES`]. Allocates.
    pub(crate) fn add(&mut self, expression: Option<&Shared<NoteExpression>>, length: f64) -> CurveRange {
        let Some(expression) = expression.filter(|e| !e.is_empty()) else { return CurveRange::default() };
        let key = (std::ptr::from_ref::<NoteExpression>(expression) as usize, length.to_bits());
        if let Some(&range) = self.prepared.get(&key) {
            return range;
        }
        let mut changes = Vec::new();
        for dim in Dim::ALL {
            sample(expression, dim, length, &mut changes);
        }
        // Stable: at one time, bend before pressure before timbre.
        changes.sort_by(|a, b| a.at.total_cmp(&b.at));
        let range = match (u32::try_from(self.changes.len()), u32::try_from(self.changes.len() + changes.len())) {
            (Ok(first), Ok(end)) if self.changes.len() + changes.len() <= MAX_CURVE_CHANGES => {
                self.changes.extend(changes);
                CurveRange { first, end }
            }
            _ => CurveRange::default(),
        };
        self.prepared.insert(key, range);
        range
    }
}

/// `dim`'s curve of `expression` as changes, for a note `length` beats long: its value at the
/// start, then at each point and every [`CURVE_STEP`] between points, where the MIDI value
/// changes. Before the first point and after the last the curve holds, so nothing changes there;
/// a curve an LFO rides on is sampled every [`CURVE_STEP`] over the whole note.
fn sample(expression: &NoteExpression, dim: Dim, length: f64, out: &mut Vec<CurveChange>) {
    let kind = dim.kind();
    let curve = expression.curve(kind);
    let mut last = dim.midi(expression.value_at(kind, 0.0));
    out.push(CurveChange { at: 0.0, dim, value: last });
    let mut emit = |offset: f64, out: &mut Vec<CurveChange>| {
        let value = dim.midi(expression.value_at(kind, offset));
        if value != last {
            out.push(CurveChange { at: offset as f32, dim, value });
            last = value;
        }
    };
    let length = if length.is_finite() { length.max(0.0) } else { 0.0 };
    // An LFO riding on the curve moves it between its points too: the whole note is sampled.
    if expression.has_lfo(kind) {
        steps(0.0, length, &mut |offset| emit(offset, out));
        return;
    }
    let mut previous: Option<f64> = None;
    for point in curve {
        if !point.offset.is_finite() || point.offset >= length {
            // The rest of the curve is past the note's end; the part up to the end is sampled.
            if let Some(from) = previous {
                steps(from, length, &mut |offset| emit(offset, out));
            }
            return;
        }
        if let Some(from) = previous {
            steps(from, point.offset, &mut |offset| emit(offset, out));
        }
        emit(point.offset, out);
        previous = Some(point.offset);
    }
}

/// Calls `at` for each offset strictly between `from` and `to`, [`CURVE_STEP`] apart from `from`.
fn steps(from: f64, to: f64, at: &mut dyn FnMut(f64)) {
    let count = ((to - from) / CURVE_STEP).ceil();
    if !(count.is_finite() && count > 1.0) {
        return;
    }
    for step in 1..(count as u64).min(u64::from(u32::MAX)) {
        let offset = from + step as f64 * CURVE_STEP;
        if offset >= to {
            break;
        }
        at(offset);
    }
}

/// Which list a sounding note's changes are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurveSource {
    /// The graph's arrangement notes'.
    Arrangement,
    /// The clip playing in a launcher column, from that slot.
    Launched { column: u32, slot: u32 },
}

/// How a note's beats become renderer frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Timing {
    /// Through the tempo map: `beat` from the note's start is renderer frame `position` +
    /// (map frame of `beat` + offset − `time`), `time` being the map frame at `position`.
    Map { beat: f64, time: u64, position: u64 },
    /// At a constant `frames_per_beat` from renderer frame `position`, the note's start.
    Rate { frames_per_beat: f64, position: u64 },
}

impl Timing {
    /// The renderer frame `at` beats from the note's start falls on (never before the note's own
    /// reference frame). Never allocates.
    fn due(self, map: &TempoMap, at: f32) -> u64 {
        match self {
            Timing::Map { beat, time, position } => {
                position.saturating_add(crate::tempo::frame_of(map, beat + f64::from(at)).saturating_sub(time))
            }
            Timing::Rate { frames_per_beat, position } => {
                let frames = f64::from(at) * frames_per_beat;
                position.saturating_add(if frames.is_finite() && frames > 0.0 { frames.round() as u64 } else { 0 })
            }
        }
    }
}

/// A starting note's curves: where they are and how its beats meet frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NoteCurve {
    pub(crate) source: CurveSource,
    pub(crate) range: CurveRange,
    pub(crate) timing: Timing,
}

/// The lists a sounding note's changes are read from, on the audio thread.
pub(crate) struct CurveLists<'a> {
    pub(crate) arrangement: &'a [CurveChange],
    pub(crate) launcher: &'a crate::launcher::ClipLauncher,
    pub(crate) map: &'a TempoMap,
}

impl CurveLists<'_> {
    fn list(&self, source: CurveSource) -> Option<&[CurveChange]> {
        match source {
            CurveSource::Arrangement => Some(self.arrangement),
            CurveSource::Launched { column, slot } => self.launcher.curves(column, slot),
        }
    }
}

/// A note's start curve with the list it is in, for [`Chain::note_on_with`].
#[derive(Clone, Copy)]
pub(crate) struct StartCurve<'a> {
    pub(crate) curve: NoteCurve,
    pub(crate) list: &'a [CurveChange],
    pub(crate) map: &'a TempoMap,
}

/// A sounding note's place in its curve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Follow {
    curve: NoteCurve,
    /// The next change to send, and the renderer frame it is due at.
    next: u32,
    due: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Member {
    /// The key sounding on it.
    key: Option<u8>,
    /// When its note started, or when it was last released (counted by [`MpeOut::clock`]).
    stamp: u64,
    /// What it was last sent: bend, pressure, timbre.
    sent: [u16; 3],
    follow: Option<Follow>,
}

const IDLE: Member = Member { key: None, stamp: 0, sent: [8192, 0, 64], follow: None };

/// A chain's channel-MPE state: fixed size, built with the chain, changed on the audio thread
/// without allocating.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MpeOut {
    /// New notes go to member channels.
    pub(crate) enabled: bool,
    /// The zone's setup has been sent since the chain was built or last stopped.
    configured: bool,
    members: [Member; MEMBERS],
    /// Per key, 1 + the member it sounds on, or 0.
    key_member: [u8; 128],
    clock: u64,
    /// The earliest frame a member's next change is due at.
    next_due: u64,
}

impl Default for MpeOut {
    fn default() -> Self {
        Self {
            enabled: false,
            configured: false,
            members: [IDLE; MEMBERS],
            key_member: [0; 128],
            clock: 0,
            next_due: NOT_HELD,
        }
    }
}

impl MpeOut {
    /// What a chain built for a new graph keeps of `old`'s: the notes sounding on member channels
    /// and what each channel was sent. Their curves are the old graph's: they stop following them
    /// until [`Chain::rebind_curve`] finds them in the new one.
    pub(crate) fn adopt(&mut self, old: &MpeOut) {
        let enabled = self.enabled;
        *self = *old;
        self.enabled = enabled;
        for member in &mut self.members {
            member.follow = None;
        }
        self.next_due = NOT_HELD;
    }

    /// Whether `key` sounds on a member channel.
    pub(crate) fn sounds(&self, key: u8) -> bool {
        self.key_member.get(usize::from(key)).is_some_and(|&m| m != 0)
    }
}

/// Whether a chain of `slots` can play as MPE: its first slot is a plug-in instrument, so the
/// member channels reach it without a MIDI effect in between.
pub(crate) fn can_play_mpe(slots: &[DeviceSlot]) -> bool {
    matches!(slots.first(), Some(DeviceSlot::Plugin(plugin)) if plugin.instrument)
}

fn controller(channel: u8, number: u8, value: u8) -> NoteEvent {
    NoteEvent::Channel { channel, message: ChannelMessage::Controller { number, value: seven_bit(u16::from(value)) } }
}

impl Chain {
    /// Plays MPE from the next note when its first slot is a plug-in instrument. Called while the
    /// graph is built.
    pub(crate) fn enable_mpe(&mut self) {
        if can_play_mpe(&self.slots) {
            self.mpe.enabled = true;
        }
    }

    /// Whether it plays its notes as MPE.
    #[cfg(test)]
    pub(crate) fn plays_mpe(&self) -> bool {
        self.mpe.enabled
    }

    /// The zone's setup at `frame`: the MPE Configuration Message on the manager channel, then on
    /// each member channel its bend range and neutral controls.
    fn mpe_setup(&mut self, frame: usize) {
        let members = MEMBERS as u8;
        // RPN 6 (MCM) on channel 1: 15 member channels; then the null RPN.
        for (number, value) in [(101, 0), (100, 6), (6, members), (101, 127), (100, 127)] {
            self.push(frame, controller(0, number, value));
        }
        for channel in 1..=members {
            // RPN 0: pitch bend sensitivity, semitones and cents; then the null RPN.
            for (number, value) in [(101, 0), (100, 0), (6, BEND_RANGE), (38, 0), (101, 127), (100, 127)] {
                self.push(frame, controller(channel, number, value));
            }
            for dim in Dim::ALL {
                self.push(frame, dim.event(channel, dim.neutral()));
            }
        }
        for member in &mut self.mpe.members {
            member.sent = IDLE.sent;
        }
        self.mpe.configured = true;
    }

    /// Starts `key` on a member channel at `frame` (renderer frame `position`): the channel's
    /// controls at the note's start first, then its note-on.
    pub(super) fn mpe_note_on(
        &mut self,
        frame: usize,
        position: u64,
        key: u8,
        velocity: f32,
        curve: Option<StartCurve<'_>>,
    ) {
        if !self.mpe.configured {
            self.mpe_setup(frame);
        }
        let member = self.free_member(frame);
        let mut values = Dim::ALL.map(Dim::neutral);
        let follow = curve.and_then(|c| catch_up(c, position, &mut values));
        self.send_values(frame, member, values);
        let channel = member as u8 + 1;
        self.push(frame, NoteEvent::Channel { channel, message: ChannelMessage::On { key, velocity } });
        self.mpe.clock += 1;
        let stamp = self.mpe.clock;
        if let Some(m) = self.mpe.members.get_mut(member) {
            m.key = Some(key);
            m.stamp = stamp;
            m.follow = follow;
        }
        if let Some(slot) = self.mpe.key_member.get_mut(usize::from(key)) {
            *slot = member as u8 + 1;
        }
        if let Some(follow) = follow {
            self.mpe.next_due = self.mpe.next_due.min(follow.due);
        }
    }

    /// A member channel for a new note: free, released longest ago; with none free, the oldest
    /// note's, released first (voice stealing).
    fn free_member(&mut self, frame: usize) -> usize {
        let members = &self.mpe.members;
        let oldest = |sounding: bool| {
            members
                .iter()
                .enumerate()
                .filter(|(_, m)| m.key.is_some() == sounding)
                .min_by_key(|(_, m)| m.stamp)
                .map(|(i, m)| (i, m.key))
        };
        match oldest(false) {
            Some((free, _)) => free,
            None => {
                let (stolen, key) = oldest(true).unwrap_or((0, None));
                if let Some(key) = key {
                    self.key_off(frame, key);
                    if let Some(held) = self.held.get_mut(usize::from(key)) {
                        *held = NOT_HELD;
                    }
                    self.arranged &= !(1u128 << key.min(127));
                }
                stolen
            }
        }
    }

    /// Sends member `member` each of `values` that differs from what it last had.
    fn send_values(&mut self, frame: usize, member: usize, values: [u16; 3]) {
        let Some(sent) = self.mpe.members.get(member).map(|m| m.sent) else { return };
        for dim in Dim::ALL {
            let (Some(&value), Some(&had)) = (values.get(dim.index()), sent.get(dim.index())) else { continue };
            if value != had {
                self.send_dim(frame, member, dim, value);
            }
        }
    }

    fn send_dim(&mut self, frame: usize, member: usize, dim: Dim, value: u16) {
        self.push(frame, dim.event(member as u8 + 1, value));
        if let Some(sent) = self.mpe.members.get_mut(member).and_then(|m| m.sent.get_mut(dim.index())) {
            *sent = value;
        }
    }

    /// Releases `key` at `frame`: on its member channel when it sounds on one, else as an
    /// ordinary note-off.
    pub(super) fn key_off(&mut self, frame: usize, key: u8) {
        let member = self.mpe.key_member.get(usize::from(key)).copied().unwrap_or(0);
        if member == 0 {
            self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
            return;
        }
        self.push(frame, NoteEvent::Channel { channel: member, message: ChannelMessage::Off { key, velocity: 0.5 } });
        if let Some(slot) = self.mpe.key_member.get_mut(usize::from(key)) {
            *slot = 0;
        }
        self.mpe.clock += 1;
        let stamp = self.mpe.clock;
        if let Some(m) = self.mpe.members.get_mut(usize::from(member) - 1) {
            m.key = None;
            m.stamp = stamp;
            m.follow = None;
        }
    }

    /// For a stop or a plug-in kept from an old chain: every member channel's notes off, and the
    /// zone's setup sent again before the next note.
    pub(super) fn mpe_all_off(&mut self, frame: usize) {
        if !self.mpe.enabled && !self.mpe.configured {
            return;
        }
        for channel in 1..=MEMBERS as u8 {
            self.push(frame, controller(channel, ALL_NOTES_OFF, 0));
        }
        self.mpe.configured = false;
    }

    /// Sends the curve changes due by renderer frame `position`, at `frame` of the block. Never
    /// allocates.
    pub(crate) fn express_due(&mut self, frame: usize, position: u64, lists: &CurveLists<'_>) {
        if position < self.mpe.next_due {
            return;
        }
        let mut next_due = NOT_HELD;
        for member in 0..MEMBERS {
            let Some(mut follow) = self.mpe.members.get(member).and_then(|m| m.follow) else { continue };
            let list = lists.list(follow.curve.source);
            let mut ended = list.is_none();
            if let Some(list) = list {
                while follow.due <= position {
                    let change =
                        (follow.next < follow.curve.range.end).then(|| list.get(follow.next as usize)).flatten();
                    let Some(&change) = change else {
                        ended = true;
                        break;
                    };
                    if self.mpe.members.get(member).and_then(|m| m.sent.get(change.dim.index())) != Some(&change.value)
                    {
                        self.send_dim(frame, member, change.dim, change.value);
                    }
                    follow.next += 1;
                    match (follow.next < follow.curve.range.end).then(|| list.get(follow.next as usize)).flatten() {
                        Some(next) => follow.due = follow.curve.timing.due(lists.map, next.at),
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
            }
            if !ended {
                next_due = next_due.min(follow.due);
            }
            if let Some(m) = self.mpe.members.get_mut(member) {
                m.follow = (!ended).then_some(follow);
            }
        }
        self.mpe.next_due = next_due;
    }

    /// After a graph swap, the note sounding on `key` follows `curve` in the new graph from
    /// renderer frame `position`: its controls catch up to where the curve is now. Never
    /// allocates.
    pub(crate) fn rebind_curve(&mut self, frame: usize, position: u64, key: u8, curve: StartCurve<'_>) {
        let member = self.mpe.key_member.get(usize::from(key)).copied().unwrap_or(0);
        let Some(member) = usize::from(member).checked_sub(1) else { return };
        let Some(mut values) = self.mpe.members.get(member).map(|m| m.sent) else { return };
        let follow = catch_up(curve, position, &mut values);
        self.send_values(frame, member, values);
        if let Some(m) = self.mpe.members.get_mut(member) {
            m.follow = follow;
        }
        if let Some(follow) = follow {
            self.mpe.next_due = self.mpe.next_due.min(follow.due);
        }
    }
}

/// Reads `curve`'s changes due by `position` into `values` (the latest of each); the place to
/// follow it from, `None` when nothing is left.
fn catch_up(curve: StartCurve<'_>, position: u64, values: &mut [u16; 3]) -> Option<Follow> {
    let StartCurve { curve, list, map } = curve;
    let mut next = curve.range.first;
    while next < curve.range.end {
        let change = list.get(next as usize)?;
        let due = curve.timing.due(map, change.at);
        if due > position {
            return Some(Follow { curve, next, due });
        }
        if let Some(value) = values.get_mut(change.dim.index()) {
            *value = change.value;
        }
        next += 1;
    }
    None
}

#[cfg(test)]
#[path = "../mpe_tests.rs"]
mod tests;
