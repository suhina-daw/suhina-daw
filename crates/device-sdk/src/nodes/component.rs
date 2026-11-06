// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Embedded immutable component definitions with private graph state.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, PreparedEffect, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
pub(crate) const COMPONENT: NodeInfo = NodeInfo {
    kind: "component",
    name: "Component",
    rust_variant: "Component",
    inputs: &[
        PortInfo { name: "in", default: Some(0.), unit: "signal" },
        PortInfo { name: "control1", default: Some(0.), unit: "signal" },
        PortInfo { name: "control2", default: Some(0.), unit: "signal" },
        PortInfo { name: "control3", default: Some(0.), unit: "signal" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Versioned embedded graph with private state and five named public inputs.",
    configuration: "name, version and embedded graph",
    example: r#"{"kind":"component","name":"Pass","version":1,"graph":"{\"format\":\"suhina-device\",\"version\":4,\"device\":{\"id\":\"org.suhina.component-pass\",\"name\":\"Pass\",\"role\":\"effect\",\"parameters\":[]},\"nodes\":[{\"id\":\"in\",\"kind\":\"audio_input\"},{\"id\":\"out\",\"kind\":\"audio_output\"}],\"connections\":[{\"from\":\"in\",\"to\":\"out\"}]}"}"#,
};
pub(crate) const COMPONENT_INPUT: NodeInfo = NodeInfo {
    kind: "component_input",
    name: "ComponentInput",
    rust_variant: "ComponentInput",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "One public component or voice-group input by stable index.",
    configuration: "index 0..4",
    example: r#"{"kind":"component_input","index":0}"#,
};
pub(crate) fn context(context: &ProcessContext) -> ProcessContext {
    ProcessContext {
        tempo: if context.tempo.is_finite() { context.tempo.clamp(1., 1000.) } else { 120. },
        beat: if context.beat.is_finite() { context.beat.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) } else { 0. },
        playing: context.playing,
    }
}
pub(crate) fn advance(context: &mut ProcessContext, frames: usize, rate: f64) {
    if context.playing {
        let step = context.tempo / (60. * rate);
        for _ in 0..frames {
            context.beat += step;
        }
    }
}
struct Component {
    graph: PreparedEffect,
    context: ProcessContext,
    rate: f64,
}
pub(crate) fn prepared(graph: PreparedEffect, rate: f64) -> Box<dyn NativeNode> {
    Box::new(Component { graph, context: ProcessContext::default(), rate })
}
impl NativeNode for Component {
    fn prepare_block(&mut self, value: &ProcessContext) {
        self.context = context(value);
        self.graph.begin_span(&self.context);
    }
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        self.graph.set_external_inputs(inputs);
        output.copy_from_slice(inputs[0]);
        self.graph.process_prepared_context(&self.context, output);
        advance(&mut self.context, output.len() / 2, self.rate);
    }
    fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.graph.parameter_range(id)
    }
    fn set_parameter(&mut self, id: &str, value: f64) {
        self.graph.set_named_parameter(id, value);
    }
    fn event(&mut self, event: NoteEvent) {
        self.graph.apply_event(event);
    }
    fn voice_finished(&self) -> Option<bool> {
        self.graph.voice_finished()
    }
    fn reset(&mut self) {
        self.graph.reset();
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
struct Input {
    index: usize,
    values: [f32; 128],
    length: usize,
    cursor: usize,
}
pub(crate) fn prepare_input(kind: &NodeKind) -> Option<Box<dyn NativeNode>> {
    if let NodeKind::ComponentInput { index } = kind {
        Some(Box::new(Input { index: usize::from(*index), values: [0.; 128], length: 0, cursor: 0 }))
    } else {
        None
    }
}
impl NativeNode for Input {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn set_external_inputs(&mut self, inputs: [&[f32]; 5]) {
        let input = inputs.get(self.index).copied().unwrap_or(&[]);
        self.length = input.len().min(self.values.len());
        self.values[..self.length].copy_from_slice(&input[..self.length]);
        self.cursor = 0;
    }
    fn process(&mut self, _: [&[f32]; 5], output: &mut [f32]) {
        for sample in output {
            *sample = self.values.get(self.cursor).filter(|_| self.cursor < self.length).copied().unwrap_or(0.);
            self.cursor = self.cursor.saturating_add(1);
        }
    }
    fn event(&mut self, _: NoteEvent) {}
    fn reset(&mut self) {
        self.length = 0;
        self.cursor = 0;
        self.values.fill(0.);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
