// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identified note occurrences shared by native instruments and format adapters (ADR 0039).
//!
//! This is an opt-in capability, separate from a saved note's editing identity. The engine's
//! legacy note queue remains unchanged until all of its consumers can preserve occurrences.

use std::num::NonZeroU64;

use crate::Device;

/// An occurrence's identity, retained from note-on until its last sounding layer finishes.
///
/// Origins are assigned uniquely when graphs or adapters are prepared. Neither field is a
/// MIDI channel, a key, an editing ID, or a host's narrower note ID. Never serialize this into
/// a project, truncate it to a host ID, or reuse it when a MIDI channel becomes free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoteInstanceId {
    origin: NonZeroU64,
    serial: NonZeroU64,
}

impl NoteInstanceId {
    /// Reconstructs a validated ID at an IPC boundary. Zero is never a valid component.
    /// The caller must also validate that the origin belongs to this connection.
    pub const fn from_parts(origin: u64, serial: u64) -> Option<Self> {
        match (NonZeroU64::new(origin), NonZeroU64::new(serial)) {
            (Some(origin), Some(serial)) => Some(Self { origin, serial }),
            _ => None,
        }
    }

    /// The origin assigned to the producer during preparation.
    pub const fn origin(self) -> u64 {
        self.origin.get()
    }

    /// The occurrence's sequence within its origin.
    pub const fn serial(self) -> u64 {
        self.serial.get()
    }
}

/// Allocation-free, nonwrapping occurrence IDs for one uniquely assigned producer origin.
///
/// This deliberately cannot be cloned or reset: duplicating its counter would duplicate IDs.
/// Rebuilding a producer needs a new origin while any old occurrence could still be sounding.
#[derive(Debug)]
pub struct NoteIdAllocator {
    origin: NonZeroU64,
    next: Option<NonZeroU64>,
}

impl NoteIdAllocator {
    /// Starts a producer's sequence at one. `origin` must be nonzero and uniquely assigned.
    pub const fn new(origin: u64) -> Option<Self> {
        match NonZeroU64::new(origin) {
            Some(origin) => Some(Self { origin, next: NonZeroU64::new(1) }),
            None => None,
        }
    }

    /// Returns a fresh ID, or `None` after exhaustion. Exhaustion never wraps or reuses IDs.
    pub fn allocate(&mut self) -> Option<NoteInstanceId> {
        let serial = self.next?;
        self.next = serial.get().checked_add(1).and_then(NonZeroU64::new);
        Some(NoteInstanceId { origin: self.origin, serial })
    }
}

/// The meaning and units of one per-note value, independent of the source protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteExpressionKind {
    /// Offset from the note's base key, in semitones, bounded to -256..=256.
    Pitch,
    /// Pressure, 0..=1; neutral is zero. A patch chooses its audible mapping.
    Pressure,
    /// Timbre, 0..=1; neutral is 0.5. A patch chooses its audible mapping.
    Timbre,
}

impl NoteExpressionKind {
    /// Rejects nonfinite input and bounds finite values to the dimension's musical range.
    pub fn normalize(self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        Some(match self {
            Self::Pitch => value.clamp(-256.0, 256.0),
            Self::Pressure | Self::Timbre => value.clamp(0.0, 1.0),
        })
    }
}

/// The complete, finite initial state supplied atomically with an identified note-on.
///
/// Values use `f64` so adapters do not unnecessarily quantize 32-bit controller values to
/// MIDI 1.0 resolution. This covers these dimensions, not arbitrary MIDI 2.0 controllers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteExpressionState {
    pitch: f64,
    pressure: f64,
    timbre: f64,
}

impl Default for NoteExpressionState {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

impl NoteExpressionState {
    /// No pitch offset or pressure, and centered timbre.
    pub const NEUTRAL: Self = Self { pitch: 0.0, pressure: 0.0, timbre: 0.5 };

    /// Creates a finite state, bounding each dimension. Rejects any nonfinite component.
    pub fn new(pitch: f64, pressure: f64, timbre: f64) -> Option<Self> {
        Some(Self {
            pitch: NoteExpressionKind::Pitch.normalize(pitch)?,
            pressure: NoteExpressionKind::Pressure.normalize(pressure)?,
            timbre: NoteExpressionKind::Timbre.normalize(timbre)?,
        })
    }

    /// Reads a dimension in its musical units.
    pub const fn value(self, kind: NoteExpressionKind) -> f64 {
        match kind {
            NoteExpressionKind::Pitch => self.pitch,
            NoteExpressionKind::Pressure => self.pressure,
            NoteExpressionKind::Timbre => self.timbre,
        }
    }

    /// Applies a bounded value; returns false, leaving all state unchanged, for nonfinite input.
    pub fn set(&mut self, kind: NoteExpressionKind, value: f64) -> bool {
        let Some(value) = kind.normalize(value) else {
            return false;
        };
        match kind {
            NoteExpressionKind::Pitch => self.pitch = value,
            NoteExpressionKind::Pressure => self.pressure = value,
            NoteExpressionKind::Timbre => self.timbre = value,
        }
        true
    }
}

/// A fixed-size event addressing one [`NoteInstanceId`].
///
/// The caller dispatches this between render spans, before the next sample, exactly like
/// [`crate::NoteEvent`]. There is no second frame offset or protocol-specific addressing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoteInstanceEvent {
    /// Starts a new occurrence with its complete initial controls, including at velocity zero.
    /// The key must be in 0..=127; finite velocity is bounded to 0..=1. MIDI 1.0 zero-velocity
    /// note-on bytes must become `Off` in the protocol adapter, before entering this contract.
    On { key: u8, velocity: f32, expression: NoteExpressionState },
    /// Releases this occurrence, not every occurrence at `key`. The carried key is metadata
    /// for explicit legacy adaptation; the identity remains authoritative in a native core.
    Off { key: u8, velocity: f32 },
    /// Changes one dimension of the addressed occurrence, including a still-sounding tail.
    Expression { kind: NoteExpressionKind, value: f64 },
    /// Immediately silences every layer of this occurrence. A choke is not a release.
    Choke { key: u8 },
}

impl NoteInstanceEvent {
    /// Validates keys and finite values and clamps finite controls without changing the event
    /// kind. Native zero-velocity note-on remains `On`; protocol-specific normalization belongs
    /// in adapters. Invalid events are ignored without changing any sounding occurrence.
    pub fn validated(self) -> Option<Self> {
        match self {
            Self::On { key, velocity, expression } if key <= 127 && velocity.is_finite() => {
                Some(Self::On { key, velocity: velocity.clamp(0.0, 1.0), expression })
            }
            Self::Off { key, velocity } if key <= 127 && velocity.is_finite() => {
                Some(Self::Off { key, velocity: velocity.clamp(0.0, 1.0) })
            }
            Self::Expression { kind, value } => Some(Self::Expression { kind, value: kind.normalize(value)? }),
            Self::Choke { key } if key <= 127 => Some(self),
            _ => None,
        }
    }
}

/// Opt-in identified-note input and final lifetime feedback for a native instrument.
///
/// This uses the same payload the planned engine queue carries; it is not an alternative
/// renderer. All methods run on the audio thread without allocation, deallocation, locks,
/// logging or I/O. Tables and finished-note storage are bounded and prepared beforehand.
pub trait NoteInstanceDevice {
    /// Applies an event at the start of the next processing span. Implementations validate
    /// input, ignore unknown IDs for expression/release, and never release another ID by key.
    /// Repeated note-on with an already sounding ID returns `AlreadyActive` without changing
    /// it. Fresh occurrences reset all controls from `On::expression`. `Capacity` rejects a
    /// new occurrence atomically when voice or retirement storage cannot accept it: no future
    /// finished notification is owed. The caller commits a new ID mapping only after `Ok`,
    /// and never removes an existing mapping on `AlreadyActive`.
    fn event(&mut self, id: NoteInstanceId, event: NoteInstanceEvent) -> Result<(), NoteInstanceDispatchError>;

    /// Writes final retirements into the beginning of `output`, returning the written count
    /// (at most its length). Each written slot is `Some(id)` and is consumed exactly once.
    /// Unwritten slots are untouched. An empty output consumes nothing. Note-off alone is not
    /// retirement: all layers must have ended or been choked. Pending IDs remain queued when
    /// the caller's buffer is small; capacity is reserved before accepting a new occurrence.
    fn drain_finished(&mut self, output: &mut [Option<NoteInstanceId>]) -> usize;
}

/// Why an identified event was not dispatched. Neither case mutates device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteInstanceDispatchError {
    /// The event contained a bad key or a nonfinite number.
    InvalidEvent,
    /// This device does not opt into identified events. The host must choose its explicit
    /// ordinary-note compatibility mode instead of silently dropping the note.
    Unsupported,
    /// Note-on repeats an ID still owned by the instrument; its existing state is unchanged.
    AlreadyActive,
    /// Release, expression or choke addressed no sounding occurrence.
    UnknownInstance,
    /// A new occurrence could not reserve its voice and eventual retirement storage.
    /// No occurrence was started and no finished notification will follow for this attempt.
    Capacity,
}

/// Fixed admission/retirement storage shared by native voice runtimes. Every accepted ID
/// reserves its completion slot up front. A finished ID is never lost when the host drains
/// slowly; new admission fails until the host consumes enough completions.
pub struct NoteLifetimes<const N: usize> {
    slots: [Option<(NoteInstanceId, bool)>; N],
}

impl<const N: usize> Default for NoteLifetimes<N> {
    fn default() -> Self {
        Self { slots: [None; N] }
    }
}

impl<const N: usize> NoteLifetimes<N> {
    /// Reserves one occurrence before changing any voice state.
    pub fn admit(&mut self, id: NoteInstanceId) -> Result<(), NoteInstanceDispatchError> {
        if self.slots.iter().flatten().any(|(existing, _)| *existing == id) {
            return Err(NoteInstanceDispatchError::AlreadyActive);
        }
        let slot = self.slots.iter_mut().find(|slot| slot.is_none()).ok_or(NoteInstanceDispatchError::Capacity)?;
        *slot = Some((id, false));
        Ok(())
    }

    /// Whether the instrument still owns this sounding occurrence.
    pub fn contains(&self, id: NoteInstanceId) -> bool {
        self.slots.iter().flatten().any(|(existing, finished)| *existing == id && !finished)
    }

    /// Marks the final layer ended. Repeated and unknown completions do nothing.
    pub fn finish(&mut self, id: NoteInstanceId) {
        if let Some((_, finished)) = self.slots.iter_mut().flatten().find(|(existing, _)| *existing == id) {
            *finished = true;
        }
    }

    /// Terminates all sounding occurrences while preserving already queued completions.
    pub fn finish_all(&mut self) {
        for (_, finished) in self.slots.iter_mut().flatten() {
            *finished = true;
        }
    }

    /// Drains up to the caller's capacity, leaving unwritten output slots unchanged.
    pub fn drain(&mut self, output: &mut [Option<NoteInstanceId>]) -> usize {
        let mut written = 0;
        for slot in &mut self.slots {
            if let Some((id, true)) = *slot {
                let Some(out) = output.get_mut(written) else {
                    break;
                };
                *out = Some(id);
                *slot = None;
                written += 1;
            }
        }
        written
    }
}

/// Validates and sends one identified event through the device's opt-in capability.
///
/// Callers must check the result. This never silently converts an identified choke into a
/// key-wide release, nor claims independent expression for a legacy device.
pub fn dispatch_note_instance(
    device: &mut dyn Device,
    id: NoteInstanceId,
    event: NoteInstanceEvent,
) -> Result<(), NoteInstanceDispatchError> {
    let event = event.validated().ok_or(NoteInstanceDispatchError::InvalidEvent)?;
    let notes = device.note_instances().ok_or(NoteInstanceDispatchError::Unsupported)?;
    notes.event(id, event)
}

#[cfg(test)]
mod tests;
