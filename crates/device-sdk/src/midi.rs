// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prepared MIDI event graph processing. Compilation allocates; processing uses caller storage.
use crate::{DeviceGraph, DeviceRole, DeviceSdkError, NodeKind, Parameter};
use suhina_device_api::NoteEvent;

#[derive(Clone, Copy)]
enum Op {
    Transpose { semitones: i8, parameter: Option<usize> },
}

enum Stage {
    Transpose(Box<PreparedMidiEffect>),
    Events(Box<dyn crate::native_node::NativeNode>),
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
    stages: Vec<Stage>,
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
        let routed = graph
            .nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::KeyRange { .. } | NodeKind::VelocityRange { .. } | NodeKind::Sustain));
        let mut stages = Vec::new();
        for index in order {
            let Some(node) = graph.nodes.get(index) else { continue };
            if routed {
                match &node.kind {
                    NodeKind::MidiTranspose { .. } | NodeKind::MidiTransposeParameter { .. } => {
                        let mut child = DeviceGraph::new("org.suhina.midi-stage", "Transpose", DeviceRole::MidiEffect);
                        child.parameters = parameters.clone();
                        child.nodes = vec![
                            crate::Node::new("in", NodeKind::MidiInput),
                            crate::Node::new("stage", node.kind.clone()),
                            crate::Node::new("out", NodeKind::MidiOutput),
                        ];
                        child.connections =
                            vec![crate::Connection::new("in", "stage"), crate::Connection::new("stage", "out")];
                        stages.push(Stage::Transpose(Box::new(Self::new(&child)?)));
                    }
                    _ => {
                        if let Some(kernel) = crate::nodes::events::prepare(&node.kind, 48000.)? {
                            stages.push(Stage::Events(kernel));
                        }
                    }
                }
                continue;
            }
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
        Ok(Self { ops, stages, parameters, values, keys: [Key::Unseen; 128] })
    }

    /// Update a prepared transpose control without allocating or rebuilding the graph.
    pub fn set_parameter(&mut self, index: usize, value: f64) {
        if let (Some(parameter), Some(current)) = (self.parameters.get(index), self.values.get_mut(index)) {
            *current = parameter.clamp(value);
            for stage in &mut self.stages {
                if let Stage::Transpose(child) = stage {
                    child.set_parameter(index, value);
                }
            }
        }
    }

    /// Transforms events in order. Returns the number written; if the output is too small,
    /// processing stops without writing beyond it. A key pressed again first ends the note it
    /// sounds; with room for only one event the release goes out and the press is dropped, so no
    /// note is left hanging. Caller should size for twice the input event cap.
    pub fn process(&mut self, input: &[(usize, NoteEvent)], output: &mut [(usize, NoteEvent)]) -> usize {
        if !self.stages.is_empty() {
            return self.process_stages(input, output);
        }
        if output.is_empty() {
            return 0;
        }
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

    fn process_stages(&mut self, input: &[(usize, NoteEvent)], output: &mut [(usize, NoteEvent)]) -> usize {
        if output.is_empty() {
            return 0;
        }
        let mut written = 0;
        for &(frame, event) in input {
            let mut batch = [(frame, NoteEvent::AllNotesOff); 256];
            batch[0] = (frame, event);
            let mut count = 1;
            for stage in &mut self.stages {
                let mut next = [(frame, NoteEvent::AllNotesOff); 256];
                count = match stage {
                    Stage::Transpose(child) => child.process(&batch[..count], &mut next),
                    Stage::Events(kernel) => {
                        let mut total = 0;
                        for &(at, event) in &batch[..count] {
                            let mut events = [NoteEvent::AllNotesOff; 256];
                            let n = kernel.route_event(event, &mut events).min(256 - total);
                            for (slot, event) in next[total..].iter_mut().zip(events).take(n) {
                                *slot = (at, event);
                            }
                            total += n;
                        }
                        total
                    }
                };
                batch = next;
            }
            let available = output.len().saturating_sub(written);
            let n = count.min(available);
            output[written..written + n].copy_from_slice(&batch[..n]);
            written += n;
            if count > available {
                if let Some(last) = output.get_mut(written.saturating_sub(1)) {
                    *last = (frame, NoteEvent::AllNotesOff);
                }
                for stage in &mut self.stages {
                    match stage {
                        Stage::Transpose(child) => {
                            let mut sink = [(0, NoteEvent::AllNotesOff); 1];
                            child.process(&[(0, NoteEvent::AllNotesOff)], &mut sink);
                        }
                        Stage::Events(kernel) => kernel.reset(),
                    }
                }
                break;
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
