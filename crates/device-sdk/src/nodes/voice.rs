// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Voice controls and explicit completion, independent of instrument DSP.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
pub(crate) const VOICE_INPUT: NodeInfo = NodeInfo {
    kind: "voice_input",
    name: "VoiceInput",
    rust_variant: "VoiceInput",
    inputs: &[PortInfo { name: "events", default: Some(0.), unit: "midi" }],
    outputs: &[
        "out",
        "left",
        "right",
        "key",
        "velocity",
        "gate",
        "trigger",
        "release",
        "bend",
        "pressure",
        "controller",
    ],
    description: "Per-voice key, gate, event pulses and channel controls.",
    configuration: "see voice contracts",
    example: r#"{"kind":"voice_input","controller":1}"#,
};
pub(crate) const MIDI_CONTROLLER: NodeInfo = NodeInfo {
    kind: "midi_controller",
    name: "MidiController",
    rust_variant: "MidiController",
    inputs: &[PortInfo { name: "events", default: Some(0.), unit: "midi" }],
    outputs: &["out", "left", "right"],
    description: "Selected normalized MIDI controller.",
    configuration: "see voice contracts",
    example: r#"{"kind":"midi_controller","number":1}"#,
};
pub(crate) const PITCH_BEND: NodeInfo = NodeInfo {
    kind: "pitch_bend",
    name: "PitchBend",
    rust_variant: "PitchBend",
    inputs: &[PortInfo { name: "events", default: Some(0.), unit: "midi" }],
    outputs: &["out", "left", "right"],
    description: "Normalized channel pitch bend.",
    configuration: "see voice contracts",
    example: r#"{"kind":"pitch_bend"}"#,
};
pub(crate) const PRESSURE: NodeInfo = NodeInfo {
    kind: "pressure",
    name: "Pressure",
    rust_variant: "Pressure",
    inputs: &[PortInfo { name: "events", default: Some(0.), unit: "midi" }],
    outputs: &["out", "left", "right"],
    description: "Normalized channel pressure.",
    configuration: "see voice contracts",
    example: r#"{"kind":"pressure"}"#,
};
pub(crate) const VOICE_OUTPUT: NodeInfo = NodeInfo {
    kind: "voice_output",
    name: "VoiceOutput",
    rust_variant: "VoiceOutput",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "finished", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Voice audio output with explicit completion and tail silence.",
    configuration: "none",
    example: r#"{"kind":"voice_output"}"#,
};
struct Control {
    kind: NodeKind,
    key: u8,
    velocity: f32,
    gate: bool,
    trigger: bool,
    release: bool,
    bend: f32,
    pressure: f32,
    controller: f32,
    finished: bool,
    signals: [[[f32; 2]; 64]; 8],
}
pub(crate) fn prepare_control(kind: &NodeKind, _: f64) -> Option<Box<dyn NativeNode>> {
    matches!(
        kind,
        NodeKind::VoiceInput { .. }
            | NodeKind::MidiController { .. }
            | NodeKind::PitchBend
            | NodeKind::Pressure
            | NodeKind::VoiceOutput
    )
    .then(|| {
        Box::new(Control {
            kind: kind.clone(),
            key: 0,
            velocity: 0.,
            gate: false,
            trigger: false,
            release: false,
            bend: 0.,
            pressure: 0.,
            controller: 0.,
            finished: false,
            signals: [[[0.; 2]; 64]; 8],
        }) as Box<dyn NativeNode>
    })
}
impl NativeNode for Control {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let values = [
                f32::from(self.key),
                self.velocity,
                f32::from(self.gate),
                f32::from(self.trigger),
                f32::from(self.release),
                self.bend,
                self.pressure,
                self.controller,
            ];
            for (port, value) in values.iter().enumerate() {
                if let Some(row) = self.signals[port].get_mut(frame) {
                    *row = [*value; 2];
                }
            }
            *pair = match self.kind {
                NodeKind::VoiceOutput => {
                    self.finished = inputs[1].get(frame * 2).is_some_and(|v| v.is_finite() && *v > 0.);
                    if self.finished {
                        [0.; 2]
                    } else {
                        std::array::from_fn(|c| inputs[0].get(frame * 2 + c).copied().unwrap_or(0.))
                    }
                }
                NodeKind::VoiceInput { .. } => [f32::from(self.key); 2],
                NodeKind::MidiController { .. } => [self.controller; 2],
                NodeKind::PitchBend => [self.bend; 2],
                NodeKind::Pressure => [self.pressure; 2],
                _ => [0.; 2],
            };
            self.trigger = false;
            self.release = false;
        }
    }
    fn output(&self, port: usize, frame: usize) -> [f32; 2] {
        self.signals.get(port).and_then(|v| v.get(frame)).copied().unwrap_or([0.; 2])
    }
    fn event(&mut self, event: NoteEvent) {
        let Some(event) = super::events::normalized(event) else {
            return;
        };
        match event {
            NoteEvent::On { key, velocity } => {
                self.key = key;
                self.velocity = velocity;
                self.gate = true;
                self.trigger = true;
            }
            NoteEvent::Off { key, .. } if self.gate && self.key == key => {
                self.gate = false;
                self.release = true;
            }
            NoteEvent::AllNotesOff => {
                self.release = self.gate;
                self.gate = false;
            }
            NoteEvent::PitchBend(v) => self.bend = v,
            NoteEvent::Pressure(v) => self.pressure = v,
            NoteEvent::Controller { number, value } => {
                let selected = match self.kind {
                    NodeKind::VoiceInput { controller } => controller,
                    NodeKind::MidiController { number } => number,
                    _ => 255,
                };
                if number == selected {
                    self.controller = value;
                }
            }
            _ => {}
        }
    }
    fn voice_finished(&self) -> Option<bool> {
        matches!(self.kind, NodeKind::VoiceOutput).then_some(self.finished)
    }
    fn reset(&mut self) {
        self.key = 0;
        self.velocity = 0.;
        self.gate = false;
        self.trigger = false;
        self.release = false;
        self.bend = 0.;
        self.pressure = 0.;
        self.controller = 0.;
        self.finished = false;
        self.signals.fill([[0.; 2]; 64]);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub(crate) const VOICE_GROUP: NodeInfo = NodeInfo {
    kind: "voice_group",
    name: "VoiceGroup",
    rust_variant: "VoiceGroup",
    inputs: &[
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
        PortInfo { name: "control1", default: Some(0.), unit: "signal" },
        PortInfo { name: "control2", default: Some(0.), unit: "signal" },
        PortInfo { name: "control3", default: Some(0.), unit: "signal" },
        PortInfo { name: "control4", default: Some(0.), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Bounded private voice graphs with sustain, retrigger, stealing and explicit completion.",
    configuration: "voices 1..32 and embedded instrument graph",
    example: r#"{"kind":"voice_group","voices":8,"graph":"{\"format\":\"suhina-device\",\"version\":4,\"device\":{\"id\":\"org.suhina.voice-example\",\"name\":\"Voice\",\"role\":\"instrument\",\"parameters\":[]},\"nodes\":[{\"id\":\"voice\",\"kind\":\"voice_input\",\"controller\":1},{\"id\":\"done\",\"kind\":\"constant\",\"value\":0.0},{\"id\":\"result\",\"kind\":\"voice_output\"},{\"id\":\"out\",\"kind\":\"audio_output\"}],\"connections\":[{\"from\":\"voice\",\"to\":\"result\"},{\"from\":\"done\",\"to\":\"result\",\"to_port\":\"finished\"},{\"from\":\"result\",\"to\":\"out\"}]}"}"#,
};

struct Voice {
    graph: crate::PreparedEffect,
    active: bool,
    held: bool,
    pending: bool,
    off_velocity: f32,
    key: u8,
    age: u64,
    last: [f32; 2],
    fade_from: [f32; 2],
    fade: usize,
}
struct Group {
    voices: Vec<Voice>,
    controllers: [f32; 128],
    bend: f32,
    pressure: f32,
    pedal: bool,
    age: u64,
    context: ProcessContext,
    rate: f64,
}
pub(crate) fn group(graphs: Vec<crate::PreparedEffect>, rate: f64) -> Box<dyn NativeNode> {
    Box::new(Group {
        voices: graphs
            .into_iter()
            .map(|graph| Voice {
                graph,
                active: false,
                held: false,
                pending: false,
                off_velocity: 0.,
                key: 0,
                age: 0,
                last: [0.; 2],
                fade_from: [0.; 2],
                fade: 0,
            })
            .collect(),
        controllers: [0.; 128],
        bend: 0.,
        pressure: 0.,
        pedal: false,
        age: 0,
        context: ProcessContext::default(),
        rate,
    })
}
impl NativeNode for Group {
    fn prepare_block(&mut self, value: &ProcessContext) {
        self.context = super::component::context(value);
        for voice in &mut self.voices {
            if voice.active {
                voice.graph.begin_span(&self.context);
            }
        }
    }
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        let mut sum = [0f64; 128];
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let mut buffer = [0.; 128];
            let count = output.len().min(buffer.len());
            voice.graph.set_external_inputs(inputs);
            voice.graph.process_prepared_context(&self.context, &mut buffer[..count]);
            for (frame, pair) in buffer[..count].as_chunks::<2>().0.iter().enumerate() {
                let fade = voice.fade as f64 / 64.;
                let mixed = std::array::from_fn::<_, 2, _>(|c| {
                    ((1. - fade) * f64::from(pair[c]) + fade * f64::from(voice.fade_from[c])) as f32
                });
                voice.fade = voice.fade.saturating_sub(1);
                voice.last = mixed;
                for c in 0..2 {
                    sum[frame * 2 + c] += f64::from(mixed[c]);
                }
            }
            if voice.graph.voice_finished() == Some(true) {
                voice.active = false;
                voice.held = false;
                voice.pending = false;
                voice.last = [0.; 2];
            }
        }
        for (sample, value) in output.iter_mut().zip(sum) {
            *sample = value.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32;
        }
        super::component::advance(&mut self.context, output.len() / 2, self.rate);
    }
    fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.voices.first().and_then(|v| v.graph.parameter_range(id))
    }
    fn set_parameter(&mut self, id: &str, value: f64) {
        for voice in &mut self.voices {
            voice.graph.set_named_parameter(id, value);
        }
    }
    fn event(&mut self, event: NoteEvent) {
        let Some(event) = super::events::normalized(event) else {
            return;
        };
        match event {
            NoteEvent::On { key, velocity } => {
                let index = self
                    .voices
                    .iter()
                    .position(|v| v.active && v.key == key)
                    .or_else(|| self.voices.iter().position(|v| !v.active))
                    .or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i));
                let Some(index) = index else {
                    return;
                };
                self.age = self.age.saturating_add(1);
                let voice = &mut self.voices[index];
                voice.fade_from = if voice.active { voice.last } else { [0.; 2] };
                voice.fade = if voice.active { 64 } else { 0 };
                voice.graph.reset();
                voice.key = key;
                voice.active = true;
                voice.held = true;
                voice.pending = false;
                voice.age = self.age;
                for (number, &value) in self.controllers.iter().enumerate() {
                    if value != 0. {
                        voice.graph.apply_event(NoteEvent::Controller { number: number as u8, value });
                    }
                }
                voice.graph.apply_event(NoteEvent::PitchBend(self.bend));
                voice.graph.apply_event(NoteEvent::Pressure(self.pressure));
                voice.graph.apply_event(NoteEvent::On { key, velocity });
            }
            NoteEvent::Off { key, velocity } => {
                for voice in &mut self.voices {
                    if voice.active && voice.key == key {
                        voice.held = false;
                        if self.pedal {
                            voice.pending = true;
                            voice.off_velocity = velocity;
                        } else {
                            voice.graph.apply_event(event);
                        }
                    }
                }
            }
            NoteEvent::Controller { number, value } => {
                self.controllers[usize::from(number)] = value;
                if number == 64 {
                    self.pedal = value >= 0.5;
                }
                for voice in &mut self.voices {
                    if voice.active {
                        voice.graph.apply_event(event);
                        if number == 64 && !self.pedal && voice.pending && !voice.held {
                            voice.pending = false;
                            voice.graph.apply_event(NoteEvent::Off { key: voice.key, velocity: voice.off_velocity });
                        }
                    }
                }
            }
            NoteEvent::PitchBend(value) => {
                self.bend = value;
                for voice in &mut self.voices {
                    if voice.active {
                        voice.graph.apply_event(event);
                    }
                }
            }
            NoteEvent::Pressure(value) => {
                self.pressure = value;
                for voice in &mut self.voices {
                    if voice.active {
                        voice.graph.apply_event(event);
                    }
                }
            }
            NoteEvent::AllNotesOff => {
                self.pedal = false;
                self.controllers[64] = 0.;
                for voice in &mut self.voices {
                    voice.held = false;
                    voice.pending = false;
                    if voice.active {
                        voice.graph.apply_event(event);
                    }
                }
            }
            _ => {
                for voice in &mut self.voices {
                    if voice.active {
                        voice.graph.apply_event(event);
                    }
                }
            }
        }
    }
    fn voice_finished(&self) -> Option<bool> {
        Some(!self.voices.iter().any(|v| v.active))
    }
    fn reset(&mut self) {
        self.controllers.fill(0.);
        self.bend = 0.;
        self.pressure = 0.;
        self.pedal = false;
        self.age = 0;
        for voice in &mut self.voices {
            voice.graph.reset();
            voice.active = false;
            voice.held = false;
            voice.pending = false;
            voice.last = [0.; 2];
            voice.fade = 0;
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
