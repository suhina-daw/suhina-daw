// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pitch: a MIDI effect that transposes every note passing through it and keeps notes inside a
//! window of keys (`docs/spec/pitch.md`).
//!
//! A note-on goes through three steps:
//!
//! 1. **Block** only: a key outside the window `[lowest, lowest + range]` is dropped. The test is
//!    on the key as played, before the transposition.
//! 2. **Transposition**: by `pitch` semitones; or, with Follow Scale on and a scale on the
//!    clip, by `pitchDegrees` steps along the scale, counted from the scale's key nearest to
//!    the played one (so a key outside the scale first moves onto it, a tie going down). With
//!    Follow Scale on and no scale, `pitchDegrees` counts semitones.
//! 3. **Fold** or **Limit**: the transposed key is brought into the window, wrapped with a period
//!    of `range + 1` keys (Fold) or moved to its nearest edge (Limit).
//!
//! The result is limited to the MIDI keys 0 to 127, so no note is lost at the ends. A note-off
//! ends the key its note-on sounded, whatever the settings are by then: the device keeps, per
//! played key, the key it sent. Velocities, timing and every other event pass unchanged.

use suhina_device_api::{
    Device, DeviceError, DeviceInfo, MusicalScale, NoteEvent, ParamInfo, ProcessContext, Role, Unit, check_sample_rate,
};

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const PITCH: usize = 0;
    pub const PITCH_DEGREES: usize = 1;
    pub const FOLLOW_SCALE: usize = 2;
    pub const LOWEST: usize = 3;
    pub const RANGE: usize = 4;
    pub const MODE: usize = 5;
    pub(crate) const COUNT: usize = 6;
}

/// What happens to a key outside the window, the [`param::MODE`] values.
pub mod mode {
    pub const BLOCK: f64 = 0.0;
    pub const FOLD: f64 = 1.0;
    pub const LIMIT: f64 = 2.0;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::integer("pitch", "Pitch", (-128, 128), 0, Unit::Semitones),
    ParamInfo::integer("pitchDegrees", "Pitch in Scale Degrees", (-30, 30), 0, Unit::ScaleDegrees),
    ParamInfo::toggle("followScale", "Follow Scale", false),
    ParamInfo::integer("lowest", "Lowest", (0, 127), 0, Unit::Key),
    ParamInfo::integer("range", "Range", (0, 127), 127, Unit::Semitones),
    ParamInfo::choice("mode", "Mode", &["Block", "Fold", "Limit"], 0),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "pitch", name: "Pitch", role: Role::MidiEffect, params: &PARAMS };

/// A played key that has no note sounding from this device.
const SILENT: u8 = u8::MAX;
/// The release velocity of a note the device ends itself (a key played again while held).
const RELEASE: f32 = 0.5;

pub struct Pitch {
    values: [f64; param::COUNT],
    /// The scale the last [`NoteEvent::Scale`] named.
    scale: Option<MusicalScale>,
    /// Per played key, the key its note sounds at, or [`SILENT`].
    sounding: [u8; 128],
}

impl Default for Pitch {
    /// A pitch effect at its default settings, as [`Pitch::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Pitch {
    /// A pitch effect at its default settings: no transposition, the whole keyboard, no scale.
    pub fn new() -> Self {
        Self { values: PARAMS.map(|p| p.default), scale: None, sounding: [SILENT; 128] }
    }

    /// The key a note-on of `key` sounds at with the current settings and scale, or `None` when
    /// Block drops it.
    pub fn transform(&self, key: u8) -> Option<u8> {
        let value = |index: usize| self.values.get(index).copied().unwrap_or_default();
        // Every value is a whole number in its range (`ParamInfo::clamp`).
        let lowest = value(param::LOWEST) as i32;
        let top = lowest + value(param::RANGE) as i32;
        let mode = value(param::MODE);
        let played = i32::from(key);
        if mode == mode::BLOCK && !(lowest..=top).contains(&played) {
            return None;
        }
        let shifted = match (value(param::FOLLOW_SCALE) >= 0.5, self.scale) {
            (true, Some(scale)) => scale.step(played, value(param::PITCH_DEGREES) as i32),
            (true, None) => played + value(param::PITCH_DEGREES) as i32,
            (false, _) => played + value(param::PITCH) as i32,
        };
        let placed = if mode == mode::FOLD {
            lowest + (shifted - lowest).rem_euclid(top - lowest + 1)
        } else if mode == mode::LIMIT {
            shifted.clamp(lowest, top)
        } else {
            shifted
        };
        u8::try_from(placed.clamp(0, 127)).ok()
    }

    /// The release of the note `key` started, if one sounds: its key, which is now free.
    fn release(&mut self, key: u8) -> Option<u8> {
        let slot = self.sounding.get_mut(usize::from(key))?;
        let sent = std::mem::replace(slot, SILENT);
        (sent != SILENT).then_some(sent)
    }
}

impl Device for Pitch {
    /// The pitch effect's description and parameters.
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    /// Checks the sample rate and forgets the scale and the sounding notes.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        check_sample_rate(sample_rate)?;
        self.reset();
        Ok(())
    }

    /// Forgets the scale and the notes that sound, without sending note-offs.
    fn reset(&mut self) {
        self.scale = None;
        self.sounding = [SILENT; 128];
    }

    /// Sets parameter `index` to `value`, limited to its range and steps.
    fn set_param(&mut self, index: usize, value: f64) {
        if let (Some(info), Some(slot)) = (PARAMS.get(index), self.values.get_mut(index)) {
            *slot = info.clamp(value);
        }
    }

    /// The value of parameter `index`.
    fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }

    /// Notes are the device's whole business; the sound passes untouched.
    fn process(&mut self, _context: &ProcessContext, _buffer: &mut [f32]) {}

    /// Moves each note-on to the key [`Pitch::transform`] gives and ends each note at the key it
    /// started, keeping room for every note-off; other events pass unchanged.
    fn process_events(
        &mut self,
        _context: &ProcessContext,
        frames: usize,
        input: &[(usize, NoteEvent)],
        output: &mut [(usize, NoteEvent)],
    ) -> usize {
        let last = frames.saturating_sub(1);
        let mut written = 0;
        // The releases still to come keep their room: a note-on never takes the place a
        // note-off needs, so a note this sends always ends (ADR 0004 › 14).
        let mut releases = input.iter().filter(|(_, event)| is_release(*event)).count();
        for &(frame, event) in input {
            if is_release(event) {
                releases = releases.saturating_sub(1);
            }
            // What this event may take once the releases after it have their room.
            let spare = output.len().saturating_sub(written).saturating_sub(releases);
            let frame = frame.min(last);
            let mut send = |event: NoteEvent| {
                if let Some(slot) = output.get_mut(written) {
                    *slot = (frame, event);
                    written += 1;
                }
            };
            match event {
                NoteEvent::On { key, velocity } if key > 127 || !velocity.is_finite() => {}
                NoteEvent::On { key, velocity } if velocity > 0.0 => {
                    let held = self.sounding.get(usize::from(key)).is_some_and(|&sent| sent != SILENT);
                    let placed = self.transform(key);
                    let needs = usize::from(held) + usize::from(placed.is_some());
                    // Without room for both, the note that sounds is ended and the new one left
                    // out; without room for either, the old note goes on until its key's note-off.
                    if held
                        && spare >= 1
                        && let Some(sent) = self.release(key)
                    {
                        send(NoteEvent::Off { key: sent, velocity: RELEASE });
                    }
                    if let Some(sent) = placed.filter(|_| spare >= needs) {
                        send(NoteEvent::On { key: sent, velocity: velocity.min(1.0) });
                        if let Some(slot) = self.sounding.get_mut(usize::from(key)) {
                            *slot = sent;
                        }
                    }
                }
                // A velocity of 0 or less is a release.
                NoteEvent::On { key, .. } => {
                    if let Some(sent) = self.release(key) {
                        send(NoteEvent::Off { key: sent, velocity: 0.0 });
                    }
                }
                NoteEvent::Off { key, velocity } => {
                    if let Some(sent) = self.release(key) {
                        let velocity = if velocity.is_finite() { velocity.clamp(0.0, 1.0) } else { RELEASE };
                        send(NoteEvent::Off { key: sent, velocity });
                    }
                }
                NoteEvent::AllNotesOff => {
                    self.sounding = [SILENT; 128];
                    send(event);
                }
                NoteEvent::Scale(scale) => {
                    self.scale = scale;
                    if spare >= 1 {
                        send(event);
                    }
                }
                NoteEvent::Controller { .. } | NoteEvent::PitchBend(_) | NoteEvent::Pressure(_) => {
                    if spare >= 1 {
                        send(event);
                    }
                }
            }
        }
        written
    }
}

/// Whether `event` ends notes: a note-off, a note-on of velocity 0 or less, or all-notes-off.
fn is_release(event: NoteEvent) -> bool {
    match event {
        NoteEvent::Off { .. } | NoteEvent::AllNotesOff => true,
        NoteEvent::On { velocity, .. } => velocity.is_finite() && velocity <= 0.0,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
