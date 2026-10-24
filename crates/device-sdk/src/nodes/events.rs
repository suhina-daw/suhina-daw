// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded event transformations, shared with instrument event routing.
use super::{NodeInfo, PortInfo};
use crate::native_node::NativeNode;
use crate::{Connection, DeviceGraph, DeviceRole, DeviceSdkError, Node, NodeKind, PreparedMidiEffect, ProcessContext};
use suhina_device_api::NoteEvent;
pub(crate) const KEY_RANGE: NodeInfo = NodeInfo {
    kind: "key_range",
    name: "KeyRange",
    rust_variant: "KeyRange",
    inputs: &[PortInfo { name: "in", default: None, unit: "midi" }],
    outputs: &["out"],
    description: "Filter note keys in an inclusive range.",
    configuration: "see event contracts",
    example: r#"{"kind":"key_range","min":0,"max":127}"#,
};
pub(crate) const VELOCITY_RANGE: NodeInfo = NodeInfo {
    kind: "velocity_range",
    name: "VelocityRange",
    rust_variant: "VelocityRange",
    inputs: &[PortInfo { name: "in", default: None, unit: "midi" }],
    outputs: &["out"],
    description: "Filter note-on velocities while preserving accepted releases.",
    configuration: "see event contracts",
    example: r#"{"kind":"velocity_range","min":0.0,"max":1.0}"#,
};
pub(crate) const SUSTAIN: NodeInfo = NodeInfo {
    kind: "sustain",
    name: "Sustain",
    rust_variant: "Sustain",
    inputs: &[PortInfo { name: "in", default: None, unit: "midi" }],
    outputs: &["out"],
    description: "Defer note releases while controller 64 is held.",
    configuration: "see event contracts",
    example: r#"{"kind":"sustain"}"#,
};

pub(crate) fn normalized(event: NoteEvent) -> Option<NoteEvent> {
    match event {
        NoteEvent::On { key, velocity } if key > 127 || !velocity.is_finite() || velocity < 0. => None,
        NoteEvent::On { key, velocity: 0. } => Some(NoteEvent::Off { key, velocity: 0. }),
        NoteEvent::On { key, velocity } => Some(NoteEvent::On { key, velocity: velocity.min(1.) }),
        NoteEvent::Off { key, velocity } if key <= 127 && velocity.is_finite() => {
            Some(NoteEvent::Off { key, velocity: velocity.clamp(0., 1.) })
        }
        NoteEvent::Off { .. } => None,
        NoteEvent::Controller { number, value } if number <= 127 && value.is_finite() => {
            Some(NoteEvent::Controller { number, value: value.clamp(0., 1.) })
        }
        NoteEvent::Controller { .. } => None,
        NoteEvent::PitchBend(v) if v.is_finite() => Some(NoteEvent::PitchBend(v.clamp(-1., 1.))),
        NoteEvent::Pressure(v) if v.is_finite() => Some(NoteEvent::Pressure(v.clamp(0., 1.))),
        NoteEvent::PitchBend(_) | NoteEvent::Pressure(_) => None,
        e => Some(e),
    }
}
struct Kernel {
    kind: NodeKind,
    accepted: [bool; 128],
    pending: [Option<NoteEvent>; 128],
    pedal: bool,
    transpose: Option<PreparedMidiEffect>,
}
pub(crate) fn prepare(kind: &NodeKind, _: f64) -> Result<Option<Box<dyn NativeNode>>, DeviceSdkError> {
    if !matches!(
        kind,
        NodeKind::KeyRange { .. } | NodeKind::VelocityRange { .. } | NodeKind::Sustain | NodeKind::MidiTranspose { .. }
    ) {
        return Ok(None);
    }
    let transpose = if matches!(kind, NodeKind::MidiTranspose { .. }) {
        let mut graph = DeviceGraph::new("org.suhina.event-transpose", "Transpose", DeviceRole::MidiEffect);
        graph.nodes = vec![
            Node::new("in", NodeKind::MidiInput),
            Node::new("transpose", kind.clone()),
            Node::new("out", NodeKind::MidiOutput),
        ];
        graph.connections = vec![Connection::new("in", "transpose"), Connection::new("transpose", "out")];
        Some(graph.compile_midi()?)
    } else {
        None
    };
    Ok(Some(Box::new(Kernel {
        kind: kind.clone(),
        accepted: [false; 128],
        pending: [None; 128],
        pedal: false,
        transpose,
    })))
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, _: [&[f32]; 5], output: &mut [f32]) {
        output.fill(0.);
    }
    fn route_event(&mut self, event: NoteEvent, output: &mut [NoteEvent]) -> usize {
        let Some(event) = normalized(event) else {
            return 0;
        };
        if let Some(transpose) = &mut self.transpose {
            let mut events = [(0, NoteEvent::AllNotesOff); 256];
            let count = transpose.process(&[(0, event)], &mut events).min(output.len());
            for (slot, (_, event)) in output.iter_mut().zip(events).take(count) {
                *slot = event;
            }
            return count;
        }
        if let NodeKind::KeyRange { min, max } = self.kind
            && matches!(event,NoteEvent::On{key,..}|NoteEvent::Off{key,..} if key<min||key>max)
        {
            return 0;
        }
        if let NodeKind::VelocityRange { min, max } = self.kind {
            match event {
                NoteEvent::On { key, velocity } => {
                    if velocity < min || velocity > max {
                        return 0;
                    }
                    self.accepted[usize::from(key)] = true;
                }
                NoteEvent::Off { key, .. } => {
                    let accepted = &mut self.accepted[usize::from(key)];
                    if !*accepted {
                        return 0;
                    }
                    *accepted = false;
                }
                NoteEvent::AllNotesOff => self.accepted.fill(false),
                _ => {}
            }
        }
        if matches!(self.kind, NodeKind::Sustain) {
            match event {
                NoteEvent::Controller { number: 64, value } => {
                    self.pedal = value >= 0.5;
                    let mut count = 0;
                    if let Some(slot) = output.get_mut(count) {
                        *slot = event;
                        count += 1;
                    }
                    if !self.pedal {
                        for pending in &mut self.pending {
                            if let Some(event) = pending.take()
                                && let Some(slot) = output.get_mut(count)
                            {
                                *slot = event;
                                count += 1;
                            }
                        }
                    }
                    return count;
                }
                NoteEvent::Off { key, .. } if self.pedal => {
                    self.pending[usize::from(key)] = Some(event);
                    return 0;
                }
                NoteEvent::On { key, .. } => self.pending[usize::from(key)] = None,
                NoteEvent::AllNotesOff => {
                    self.pending.fill(None);
                    self.pedal = false;
                }
                _ => {}
            }
        }
        if let Some(slot) = output.first_mut() {
            *slot = event;
            1
        } else {
            0
        }
    }
    fn event(&mut self, _: NoteEvent) {}
    fn reset(&mut self) {
        self.accepted.fill(false);
        self.pending.fill(None);
        self.pedal = false;
        if let Some(transpose) = &mut self.transpose {
            let mut events = [(0, NoteEvent::AllNotesOff); 256];
            let _ = transpose.process(&[(0, NoteEvent::AllNotesOff)], &mut events);
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
