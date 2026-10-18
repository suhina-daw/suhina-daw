// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prepared MIDI event graph processing. Compilation allocates; processing uses caller storage.
use crate::{DeviceGraph, DeviceRole, DeviceSdkError, NodeKind, Parameter};
use suhina_device_api::NoteEvent;

#[derive(Clone, Copy)]
enum Op {
    Transpose { semitones: i8, parameter: Option<usize> },
}

/// What a MIDI effect knows of one input key.
#[derive(Clone, Copy, PartialEq)]
enum Key {
    /// No note-on for it has passed since the effect was made: a release is mapped as it comes,
    /// so a note that started before the effect (a chain rebuilt while a key is down) still ends.
    Unseen,
    /// Sounding downstream as this key.
    Held(u8),
    /// Released, or its note-on fell outside the key range: a release has nothing to end.
    Idle,
}

/// A compiled MIDI effect. Keep one output buffer allocated outside the audio callback and
/// pass it to [`process`](Self::process); event offsets are preserved exactly.
pub struct PreparedMidiEffect {
    ops: Vec<Op>,
    parameters: Vec<Parameter>,
    values: Vec<f64>,
    keys: [Key; 128],
}

impl PreparedMidiEffect {
    pub(crate) fn new(graph: &DeviceGraph) -> Result<Self, DeviceSdkError> {
        if graph.role != DeviceRole::MidiEffect {
            return Err(DeviceSdkError("expected a midi_effect graph".into()));
        }
        let order = graph.execution_order()?;
        let mut ops = Vec::with_capacity(graph.nodes.len());
        let parameters = graph.parameters.clone();
        let values = parameters.iter().map(|parameter| parameter.default).collect();
        for index in order {
            let Some(node) = graph.nodes.get(index) else { continue };
            match &node.kind {
                NodeKind::MidiTranspose { semitones } => {
                    ops.push(Op::Transpose { semitones: *semitones, parameter: None });
                }
                NodeKind::MidiTransposeParameter { parameter } => {
                    let index = parameters.iter().position(|p| p.id == *parameter).ok_or_else(|| {
                        DeviceSdkError(format!("node {:?}: missing parameter {parameter:?}", node.id))
                    })?;
                    ops.push(Op::Transpose { semitones: 0, parameter: Some(index) });
                }
                _ => {}
            }
        }
        Ok(Self { ops, parameters, values, keys: [Key::Unseen; 128] })
    }

    /// Update a prepared transpose control without allocating or rebuilding the graph.
    pub fn set_parameter(&mut self, index: usize, value: f64) {
        if let (Some(parameter), Some(current)) = (self.parameters.get(index), self.values.get_mut(index)) {
            *current = parameter.clamp(value);
        }
    }

    /// Transforms events in order. Returns the number written; if the output is too small,
    /// processing stops without writing beyond it. A key pressed again first ends the note it
    /// sounds; with room for only one event the release goes out and the press is dropped, so no
    /// note is left hanging. Caller should size for twice the input event cap.
    pub fn process(&mut self, input: &[(usize, NoteEvent)], output: &mut [(usize, NoteEvent)]) -> usize {
        let mut written = 0;
        for &(frame, event) in input {
            let Some(slot) = output.get_mut(written) else { break };
            let event = match event {
                NoteEvent::On { key, velocity: 0.0 } => NoteEvent::Off { key, velocity: 0.0 },
                event => event,
            };
            match event {
                NoteEvent::Off { key, velocity } => {
                    if let Some(mapped) = self.release(key) {
                        *slot = (frame, NoteEvent::Off { key: mapped, velocity });
                        written += 1;
                    }
                }
                NoteEvent::On { key, velocity } => {
                    if let Some(Key::Held(mapped)) = self.keys.get(usize::from(key)).copied() {
                        *slot = (frame, NoteEvent::Off { key: mapped, velocity: 0.0 });
                        written += 1;
                        self.set_key(key, Key::Idle);
                    }
                    let Some(slot) = output.get_mut(written) else { break };
                    let mapped = self.transpose(key);
                    self.set_key(key, mapped.map_or(Key::Idle, Key::Held));
                    if let Some(mapped) = mapped {
                        *slot = (frame, NoteEvent::On { key: mapped, velocity });
                        written += 1;
                    }
                }
                NoteEvent::AllNotesOff => {
                    self.keys.fill(Key::Idle);
                    *slot = (frame, event);
                    written += 1;
                }
                event => {
                    *slot = (frame, event);
                    written += 1;
                }
            }
        }
        written
    }

    /// The downstream key a release of `key` ends, if any, and forgets it.
    fn release(&mut self, key: u8) -> Option<u8> {
        match self.keys.get(usize::from(key)).copied()? {
            Key::Held(mapped) => {
                self.set_key(key, Key::Idle);
                Some(mapped)
            }
            Key::Idle => None,
            Key::Unseen => self.transpose(key),
        }
    }

    fn set_key(&mut self, key: u8, state: Key) {
        if let Some(current) = self.keys.get_mut(usize::from(key)) {
            *current = state;
        }
    }

    /// `key` through every transpose step, or `None` once a step leaves the key range.
    fn transpose(&self, key: u8) -> Option<u8> {
        let mut key = i16::from(key);
        for op in &self.ops {
            let Op::Transpose { semitones, parameter } = *op;
            let semitones = parameter
                .and_then(|index| self.values.get(index))
                .map_or(f64::from(semitones), |value| *value)
                .round()
                .clamp(f64::from(i8::MIN), f64::from(i8::MAX)) as i8;
            key = key.checked_add(i16::from(semitones)).filter(|k| (0..=127).contains(k))?;
        }
        u8::try_from(key).ok()
    }
}
