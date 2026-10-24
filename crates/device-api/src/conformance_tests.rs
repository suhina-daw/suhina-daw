// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{DeviceError, DeviceInfo, ParamInfo, Role, Scale, Unit};

const PARAMS: [ParamInfo; 1] = [ParamInfo::continuous("gain", "Gain", (0.0, 2.0), 1.0, Unit::None, Scale::Linear)];
const INFO: DeviceInfo = DeviceInfo { id: "broken", name: "Broken", role: Role::Effect, params: &PARAMS };

/// Breaks the contract in two ways: it applies a gain change once per block (so the output
/// depends on the block size) and keeps state across `reset`.
struct Broken {
    gain: f64,
    applied: f32,
    counter: f32,
}

impl Device for Broken {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        crate::check_sample_rate(sample_rate).map(|_| ())
    }
    fn reset(&mut self) {}
    fn set_param(&mut self, index: usize, value: f64) {
        if index == 0 {
            self.gain = PARAMS[0].clamp(value);
        }
    }
    fn param(&self, index: usize) -> Option<f64> {
        (index == 0).then_some(self.gain)
    }
    fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
        self.applied += (self.gain as f32 - self.applied) * 0.5;
        for s in buffer.as_chunks_mut::<2>().0.iter_mut().flatten() {
            self.counter += 1e-3;
            *s *= self.applied + self.counter.sin() * 0.01;
        }
    }
}

#[test]
fn catches_a_device_that_breaks_the_contract() {
    let failures = check(&|| Box::new(Broken { gain: 1.0, applied: 1.0, counter: 0.0 }));
    assert!(failures.iter().any(|f| f.contains("blocks of")), "{failures:#?}");
    assert!(failures.iter().any(|f| f.contains("after reset")), "{failures:#?}");
}

/// Delays its input by [`Late::DELAY`] frames, reports `reports` frames of latency, and
/// clears its delay line on `prepare` only when `clears` is set.
struct Late {
    line: [[f32; 2]; Late::DELAY],
    at: usize,
    reports: u32,
    clears: bool,
}

impl Late {
    const DELAY: usize = 64;

    fn new(reports: u32, clears: bool) -> Self {
        Self { line: [[0.0; 2]; Self::DELAY], at: 0, reports, clears }
    }
}

const LATE: DeviceInfo = DeviceInfo { id: "late", name: "Late", role: Role::Effect, params: &[] };

impl Device for Late {
    fn info(&self) -> &'static DeviceInfo {
        &LATE
    }
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        crate::check_sample_rate(sample_rate)?;
        if self.clears {
            self.reset();
        }
        Ok(())
    }
    fn reset(&mut self) {
        *self = Self::new(self.reports, self.clears);
    }
    fn set_param(&mut self, _: usize, _: f64) {}
    fn param(&self, _: usize) -> Option<f64> {
        None
    }
    fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            let input = if frame.iter().all(|s| s.is_finite()) { *frame } else { [0.0; 2] };
            *frame = std::mem::replace(&mut self.line[self.at], input);
            self.at = (self.at + 1) % Self::DELAY;
        }
    }
    fn latency(&self) -> u32 {
        self.reports
    }
}

/// An effect that reports the delay it adds and clears it when prepared passes; one that
/// reports none, or keeps its delay line across `prepare`, does not.
#[test]
fn catches_unreported_latency_and_state_kept_across_prepare() {
    assert_eq!(check(&|| Box::new(Late::new(64, true))), Vec::<String>::new());
    let failures = check(&|| Box::new(Late::new(0, true)));
    assert!(failures.iter().any(|f| f.contains("first heard at frame 64, but latency() is 0")), "{failures:#?}");
    let failures = check(&|| Box::new(Late::new(64, false)));
    assert!(failures.iter().any(|f| f.contains("prepared at 96 kHz")), "{failures:#?}");
}

const MIDI: DeviceInfo = DeviceInfo { id: "sticky", name: "Sticky", role: Role::MidiEffect, params: &PARAMS };

/// A MIDI effect that transposes by its gain in octaves and ends a note at the key the
/// current gain gives, so a change while a note is held leaves it hanging.
struct Sticky {
    gain: f64,
}

impl Device for Sticky {
    /// The broken MIDI effect's description.
    fn info(&self) -> &'static DeviceInfo {
        &MIDI
    }
    /// Accepts any sample rate the checks accept.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        crate::check_sample_rate(sample_rate).map(|_| ())
    }
    /// Holds nothing to reset: the fault is in how it ends notes.
    fn reset(&mut self) {}
    /// Sets the transposition, in octaves.
    fn set_param(&mut self, index: usize, value: f64) {
        if index == 0 {
            self.gain = PARAMS[0].clamp(value);
        }
    }
    /// The transposition, in octaves.
    fn param(&self, index: usize) -> Option<f64> {
        (index == 0).then_some(self.gain)
    }
    /// Leaves the sound as it is.
    fn process(&mut self, _: &ProcessContext, _: &mut [f32]) {}
    /// Transposes note-ons and note-offs alike by the current setting, so a note-off after a change
    /// misses the key its note-on sent.
    fn process_events(
        &mut self,
        _: &ProcessContext,
        _: usize,
        input: &[(usize, NoteEvent)],
        output: &mut [(usize, NoteEvent)],
    ) -> usize {
        let shift = (self.gain * 12.0) as u8;
        let mut written = 0;
        for (&(at, event), slot) in input.iter().zip(output.iter_mut()) {
            *slot = match event {
                NoteEvent::On { key, velocity } => {
                    (at, NoteEvent::On { key: key.saturating_add(shift).min(127), velocity })
                }
                NoteEvent::Off { key, velocity } => {
                    (at, NoteEvent::Off { key: key.saturating_add(shift).min(127), velocity })
                }
                other => (at, other),
            };
            written += 1;
        }
        written
    }
}

/// The checks catch a MIDI effect whose note-off goes to a different key than its note-on after a
/// parameter change.
#[test]
fn catches_a_midi_effect_that_leaves_notes_hanging() {
    let failures = check(&|| Box::new(Sticky { gain: 1.0 }));
    assert!(failures.iter().any(|f| f.contains("still sounds")), "{failures:#?}");
}

/// The default `process_events` passes every event on, which the checks accept.
struct Through;

impl Device for Through {
    /// The pass-through MIDI effect's description.
    fn info(&self) -> &'static DeviceInfo {
        &MIDI
    }
    /// Accepts any sample rate the checks accept.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        crate::check_sample_rate(sample_rate).map(|_| ())
    }
    /// Holds nothing to reset.
    fn reset(&mut self) {}
    /// Ignores its one parameter.
    fn set_param(&mut self, _: usize, _: f64) {}
    /// Its one parameter, always 1.
    fn param(&self, index: usize) -> Option<f64> {
        (index == 0).then_some(1.0)
    }
    /// Leaves the sound as it is.
    fn process(&mut self, _: &ProcessContext, _: &mut [f32]) {}
}

/// A MIDI effect that keeps the default `process_events` fails none of the event checks.
#[test]
fn a_midi_effect_that_passes_events_through_passes() {
    let failures = check(&|| Box::new(Through));
    let events: Vec<&String> = failures.iter().filter(|f| !f.contains("parameter")).collect();
    assert!(events.is_empty(), "{events:#?}");
}

/// A MIDI effect that doubles every note an octave up and writes until its output is full,
/// so with too little room the note-offs at the end are the ones left out; or, `swallow`,
/// one that drops all-notes-off and forgets nothing.
struct Doubler {
    swallow: bool,
}

impl Device for Doubler {
    /// The doubling MIDI effect's description.
    fn info(&self) -> &'static DeviceInfo {
        &MIDI
    }
    /// Accepts any sample rate the checks accept.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        crate::check_sample_rate(sample_rate).map(|_| ())
    }
    /// Holds nothing to reset.
    fn reset(&mut self) {}
    /// Ignores its one parameter.
    fn set_param(&mut self, _: usize, _: f64) {}
    /// Its one parameter, always 1.
    fn param(&self, index: usize) -> Option<f64> {
        (index == 0).then_some(1.0)
    }
    /// Leaves the sound as it is.
    fn process(&mut self, _: &ProcessContext, _: &mut [f32]) {}
    /// Doubles each note an octave up until the output is full, or with `swallow` drops
    /// all-notes-off and passes the rest.
    fn process_events(
        &mut self,
        _: &ProcessContext,
        _: usize,
        input: &[(usize, NoteEvent)],
        output: &mut [(usize, NoteEvent)],
    ) -> usize {
        let mut written = 0;
        let mut send = |event| {
            if let Some(slot) = output.get_mut(written) {
                *slot = event;
                written += 1;
            }
        };
        for &(at, event) in input {
            match event {
                NoteEvent::AllNotesOff if self.swallow => {}
                NoteEvent::On { key, velocity } if !self.swallow && key < 116 => {
                    send((at, event));
                    send((at, NoteEvent::On { key: key + 12, velocity }));
                }
                NoteEvent::Off { key, velocity } if !self.swallow && key < 116 => {
                    send((at, event));
                    send((at, NoteEvent::Off { key: key + 12, velocity }));
                }
                other => send((at, other)),
            }
        }
        written
    }
}

/// The checks catch a MIDI effect that fills its output with note-ons and leaves no room for the
/// note-offs, without blaming it for the hostile events.
#[test]
fn catches_a_midi_effect_that_spends_the_room_note_offs_need() {
    let failures = check(&|| Box::new(Doubler { swallow: false }));
    assert!(failures.iter().any(|f| f.contains("room for as many") && f.contains("still sounds")), "{failures:#?}");
    assert!(!failures.iter().any(|f| f.starts_with("hostile events: ")), "{failures:#?}");
}

/// The checks catch a MIDI effect that swallows all-notes-off.
#[test]
fn catches_a_midi_effect_that_drops_all_notes_off() {
    let failures = check(&|| Box::new(Doubler { swallow: true }));
    assert!(failures.iter().any(|f| f.starts_with("all-notes-off:")), "{failures:#?}");
}
