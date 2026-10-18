// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Events that go with a block: notes, controllers and parameter values, each at a frame of the
//! block. They are stored in shared memory as fixed 16-byte records, so neither side allocates
//! to pass them: frame (u32), kind (u8), channel (u8), key or controller number (u8), a zero
//! byte, value (f32: velocity, controller value, bend or parameter value), note id (i32, -1 for
//! none; a parameter event's parameter id); little-endian.

/// Most events one block carries; more are dropped (the latest ones).
pub const MAX_EVENTS: usize = 1024;
/// Bytes of one stored event.
pub(crate) const EVENT_BYTES: usize = 16;

/// What happens at an event's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind {
    /// A note starts. `velocity` is 0 to 1.
    NoteOn { channel: u8, key: u8, velocity: f32 },
    /// A note ends. `velocity` is the release velocity, 0 to 1.
    NoteOff { channel: u8, key: u8, velocity: f32 },
    /// A MIDI controller moves to `value`, 0 to 1.
    Controller { channel: u8, number: u8, value: f32 },
    /// The pitch bend moves to `value`, -1 to 1.
    PitchBend { channel: u8, value: f32 },
    /// Plug-in parameter `id` moves to `value`, in its own range (as `get_params` gives it),
    /// from this frame on: automation.
    Param { id: u32, value: f32 },
    /// Where the song is as the block starts: whether the transport runs and the arrangement
    /// position of the block's first frame, in frames. Sent at frame 0 of every block of a
    /// plug-in that follows the song (an ARA renderer, ADR 0028); a host that does not know the
    /// kind skips it.
    Transport { playing: bool, position: u64 },
    /// The tempo as the block starts, in beats per minute, beside [`EventKind::Transport`].
    Tempo { bpm: f32 },
}

/// One event at `frame` of the block (0 is its first frame). `note` ties a note's start and end
/// together when several notes share a key; -1 for none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub frame: u32,
    pub note: i32,
    pub kind: EventKind,
}

impl Event {
    pub fn note_on(frame: u32, channel: u8, key: u8, velocity: f32) -> Self {
        Self { frame, note: -1, kind: EventKind::NoteOn { channel, key, velocity } }
    }

    pub fn note_off(frame: u32, channel: u8, key: u8, velocity: f32) -> Self {
        Self { frame, note: -1, kind: EventKind::NoteOff { channel, key, velocity } }
    }

    /// A parameter value at `frame`.
    pub fn param(frame: u32, id: u32, value: f32) -> Self {
        Self { frame, note: -1, kind: EventKind::Param { id, value } }
    }

    /// The transport at the start of a block ([`EventKind::Transport`]).
    pub fn transport(playing: bool, position: u64) -> Self {
        Self { frame: 0, note: -1, kind: EventKind::Transport { playing, position } }
    }

    /// The tempo at the start of a block ([`EventKind::Tempo`]).
    pub fn tempo(bpm: f32) -> Self {
        Self { frame: 0, note: -1, kind: EventKind::Tempo { bpm } }
    }

    pub(crate) fn encode(&self) -> [u8; EVENT_BYTES] {
        let mut note = self.note;
        let (kind, channel, number, value) = match self.kind {
            EventKind::NoteOn { channel, key, velocity } => (1, channel, key, velocity),
            EventKind::NoteOff { channel, key, velocity } => (2, channel, key, velocity),
            EventKind::Controller { channel, number, value } => (3, channel, number, value),
            EventKind::PitchBend { channel, value } => (4, channel, 0, value),
            EventKind::Param { id, value } => {
                note = i32::from_le_bytes(id.to_le_bytes());
                (5, 0, 0, value)
            }
            // The position's low half in the value's bytes, its high half in the note's.
            EventKind::Transport { playing, position } => {
                let [l0, l1, l2, l3, h0, h1, h2, h3] = position.to_le_bytes();
                note = i32::from_le_bytes([h0, h1, h2, h3]);
                (6, u8::from(playing), 0, f32::from_le_bytes([l0, l1, l2, l3]))
            }
            EventKind::Tempo { bpm } => (7, 0, 0, bpm),
        };
        let mut out = [0; EVENT_BYTES];
        out[..4].copy_from_slice(&self.frame.to_le_bytes());
        out[4] = kind;
        out[5] = channel;
        out[6] = number;
        out[8..12].copy_from_slice(&value.to_le_bytes());
        out[12..].copy_from_slice(&note.to_le_bytes());
        out
    }

    /// The event in `bytes`, or `None` for a kind this version does not know. Numbers are kept
    /// in range: channel 0–15, key 0–127, values within their span (NaN becomes 0).
    pub(crate) fn decode(bytes: &[u8; EVENT_BYTES]) -> Option<Self> {
        let [f0, f1, f2, f3, kind, channel, number, _, v0, v1, v2, v3, n0, n1, n2, n3] = *bytes;
        let value = f32::from_le_bytes([v0, v1, v2, v3]);
        // NaN becomes 0 and infinities the largest finite numbers, so no plug-in gets them.
        let value = if value.is_nan() { 0.0 } else { value.clamp(f32::MIN, f32::MAX) };
        let (channel, number) = (channel.min(15), number.min(127));
        let unit = value.clamp(0.0, 1.0);
        let kind = match kind {
            1 => EventKind::NoteOn { channel, key: number, velocity: unit },
            2 => EventKind::NoteOff { channel, key: number, velocity: unit },
            3 => EventKind::Controller { channel, number, value: unit },
            4 => EventKind::PitchBend { channel, value: value.clamp(-1.0, 1.0) },
            6 => {
                let position = u64::from_le_bytes([v0, v1, v2, v3, n0, n1, n2, n3]);
                let kind = EventKind::Transport { playing: channel != 0, position };
                return Some(Self { frame: 0, note: -1, kind });
            }
            7 => {
                let bpm = if value.is_finite() && value > 0.0 { value.min(999.0) } else { 120.0 };
                return Some(Self { frame: 0, note: -1, kind: EventKind::Tempo { bpm } });
            }
            5 => {
                let id = u32::from_le_bytes([n0, n1, n2, n3]);
                let event = Self {
                    frame: u32::from_le_bytes([f0, f1, f2, f3]),
                    note: -1,
                    kind: EventKind::Param { id, value },
                };
                return Some(event);
            }
            _ => return None,
        };
        Some(Self { frame: u32::from_le_bytes([f0, f1, f2, f3]), note: i32::from_le_bytes([n0, n1, n2, n3]), kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_and_stay_in_range() {
        let events = [
            Event::note_on(0, 0, 60, 0.75),
            Event { frame: 511, note: 42, kind: EventKind::NoteOff { channel: 15, key: 127, velocity: 0.0 } },
            Event { frame: 3, note: -1, kind: EventKind::Controller { channel: 2, number: 64, value: 1.0 } },
            Event { frame: 4, note: -1, kind: EventKind::PitchBend { channel: 1, value: -0.5 } },
            Event::param(7, 0, 0.25),
            Event::param(8, u32::MAX, -440.0),
            Event::transport(true, 0),
            Event::transport(false, u64::MAX),
            // A position whose low half is a NaN bit pattern still comes back whole.
            Event::transport(true, 0x7FC0_0001_7FC0_0001),
            Event::tempo(98.5),
        ];
        for event in events {
            assert_eq!(Event::decode(&event.encode()), Some(event));
        }
        let mut wild = Event::note_on(1, 200, 250, f32::NAN).encode();
        assert_eq!(Event::decode(&wild), Some(Event::note_on(1, 15, 127, 0.0)));
        wild[8..12].copy_from_slice(&9.0f32.to_le_bytes());
        assert_eq!(Event::decode(&wild), Some(Event::note_on(1, 15, 127, 1.0)));
        wild[4] = 9;
        assert_eq!(Event::decode(&wild), None);
        // A tempo that is not a tempo reads as 120.
        assert_eq!(Event::decode(&Event::tempo(f32::NAN).encode()), Some(Event::tempo(120.0)));
        assert_eq!(Event::decode(&Event::tempo(-1.0).encode()), Some(Event::tempo(120.0)));
        let endless = Event::decode(&Event::param(0, 3, f32::INFINITY).encode());
        assert_eq!(endless, Some(Event::param(0, 3, f32::MAX)));
    }
}
