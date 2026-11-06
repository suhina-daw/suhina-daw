// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Graph trust boundary and dependency scheduling, never called on the audio thread.
use crate::{DeviceGraph, DeviceRole, DeviceSdkError, MAX_DELAY_FRAMES, MAX_TEXT_BYTES, NodeKind};
use std::collections::HashSet;

impl DeviceGraph {
    pub(crate) fn validate_nodes(&self) -> Result<(), DeviceSdkError> {
        if self.nodes.iter().map(|node| node.bindings.len()).sum::<usize>() > crate::MAX_CONNECTIONS {
            return Err(DeviceSdkError("too many graph parameter bindings".into()));
        }
        let mut ids = HashSet::new();
        let mut inputs = 0;
        let mut outputs = 0;
        let midi = self.role == DeviceRole::MidiEffect;
        let instrument = self.role == DeviceRole::Instrument;
        let mut midi_sources = 0;
        for node in &self.nodes {
            let fail = |message: &str| DeviceSdkError(format!("node {:?}: {message}", node.id));
            if node.id.is_empty() || node.id.len() > MAX_TEXT_BYTES || !ids.insert(&node.id) {
                return Err(fail("id must be unique and 1..128 bytes"));
            }
            if midi
                && !matches!(
                    node.kind,
                    NodeKind::MidiInput
                        | NodeKind::MidiOutput
                        | NodeKind::MidiTranspose { .. }
                        | NodeKind::MidiTransposeParameter { .. }
                        | NodeKind::KeyRange { .. }
                        | NodeKind::VelocityRange { .. }
                        | NodeKind::Sustain
                )
            {
                return Err(fail(
                    "only MIDI inputs, outputs, transposes, ranges and sustain are supported in midi_effect graphs",
                ));
            }
            if let NodeKind::VoiceGroup { graph, voices } = &node.kind
                && (graph.len() > crate::MAX_FILE_BYTES || *voices == 0 || *voices > 32)
            {
                return Err(fail("voice groups need a bounded graph and 1..32 voices"));
            }
            if let NodeKind::GrainPool { graph, grains } = &node.kind
                && (graph.len() > crate::MAX_FILE_BYTES || *grains == 0 || *grains > 128)
            {
                return Err(fail("grain pools need a bounded graph and 1..128 grains"));
            }
            if let NodeKind::GranularPlayer { voices, grains, .. } = &node.kind
                && (*voices == 0
                    || *voices > 32
                    || *grains == 0
                    || *grains > 128
                    || usize::from(*voices) * usize::from(*grains) > crate::nodes::factory::MAX_GRAINS_IN_ALL)
            {
                // Each voice holds its own grains, so preparing it would refuse more than this in all.
                return Err(fail("granular players need 1..32 voices, 1..128 grains and at most 256 grains in all"));
            }
            if node.kind.is_instrument_event_node()
                && !instrument
                && !(midi
                    && matches!(
                        node.kind,
                        NodeKind::KeyRange { .. } | NodeKind::VelocityRange { .. } | NodeKind::Sustain
                    ))
            {
                return Err(fail("voice/event nodes require an instrument graph"));
            }
            if instrument && matches!(node.kind, NodeKind::MidiKey | NodeKind::MidiVelocity | NodeKind::MidiGate) {
                midi_sources += 1;
            } else if !instrument
                && matches!(node.kind, NodeKind::MidiKey | NodeKind::MidiVelocity | NodeKind::MidiGate)
            {
                return Err(fail("MIDI value nodes are only valid in instrument graphs"));
            }
            if node.bindings.len() > crate::MAX_PARAMETERS {
                return Err(fail("too many parameter bindings"));
            }
            let mut targets = HashSet::new();
            for binding in &node.bindings {
                if binding.target.is_empty()
                    || binding.target.len() > 128
                    || !targets.insert(&binding.target)
                    || !self.parameters.iter().any(|p| p.id == binding.parameter)
                {
                    return Err(fail("invalid parent/target parameter binding"));
                }
            }
            if node.kind.auxiliary_count() > 8 {
                return Err(fail("too many named outputs"));
            }
            match &node.kind {
                NodeKind::SamplerAsset { sample }
                | NodeKind::SampleAsset { sample }
                | NodeKind::SliceMap { sample, .. }
                | NodeKind::SlicePlayback { sample, .. }
                | NodeKind::TimeStretchPlayer { sample }
                | NodeKind::GranularPlayer { sample, .. }
                    if sample.is_empty() || sample.len() > MAX_TEXT_BYTES =>
                {
                    return Err(fail("sample binding must be 1..128 bytes"));
                }
                NodeKind::SliceMap { boundaries, .. }
                    if boundaries.len() < 2
                        || boundaries.len() > 4096
                        || boundaries.windows(2).any(|p| p[0] >= p[1]) =>
                {
                    return Err(fail("slice maps need 2..4096 strictly increasing frame boundaries"));
                }
                NodeKind::KeyRange { min, max } if min > max || *max > 127 => {
                    return Err(fail("key range must be ordered within 0..127"));
                }
                NodeKind::VelocityRange { min, max }
                    if !min.is_finite() || !max.is_finite() || *min < 0. || *max > 1. || min > max =>
                {
                    return Err(fail("velocity range must be finite, ordered and within 0..1"));
                }
                NodeKind::VoiceInput { controller } | NodeKind::MidiController { number: controller }
                    if *controller > 127 =>
                {
                    return Err(fail("controller must be 0..127"));
                }
                NodeKind::GrainRead
                | NodeKind::TimeStretchPlayer { .. }
                | NodeKind::GranularPlayer { .. }
                | NodeKind::SlicePlayback { .. }
                | NodeKind::VoiceInput { .. }
                | NodeKind::VoiceGroup { .. }
                | NodeKind::MidiController { .. }
                | NodeKind::PitchBend
                | NodeKind::Pressure
                    if instrument =>
                {
                    midi_sources += 1
                }
                NodeKind::VoiceGroup { graph, voices }
                    if graph.len() > crate::MAX_FILE_BYTES || *voices == 0 || *voices > 32 =>
                {
                    return Err(fail("voice groups need a bounded graph and 1..32 voices"));
                }
                NodeKind::Component { name, version, graph }
                    if name.is_empty() || name.len() > 128 || *version == 0 || graph.len() > crate::MAX_FILE_BYTES =>
                {
                    return Err(fail("component name, version or graph is invalid"));
                }
                NodeKind::ComponentInput { index } if *index > 4 => {
                    return Err(fail("component input index must be 0..4"));
                }
                NodeKind::MidiInput if instrument => midi_sources += 1,
                NodeKind::MidiTranspose { .. } if instrument => {}
                NodeKind::Sampler | NodeKind::SamplerAsset { .. } if instrument => midi_sources += 1,
                NodeKind::Sampler | NodeKind::SamplerAsset { .. } => {
                    return Err(fail("sampler is only valid in instrument graphs"));
                }
                NodeKind::NativeEffect { effect } => {
                    if effect.is_empty() || effect.len() > MAX_TEXT_BYTES {
                        return Err(fail("native effect id must be 1..128 bytes"));
                    }
                }
                NodeKind::AudioInput if !instrument => inputs += 1,
                NodeKind::AudioInput => return Err(fail("instrument graphs have no audio_input")),
                NodeKind::AudioOutput => outputs += 1,
                NodeKind::MidiInput if midi => inputs += 1,
                NodeKind::MidiOutput if midi => outputs += 1,
                NodeKind::MidiTransposeParameter { parameter } if midi => {
                    if !self.parameters.iter().any(|p| p.id == *parameter) {
                        return Err(fail(&format!("unknown parameter {parameter:?}")));
                    }
                }
                NodeKind::MidiTranspose { .. } if midi => {}
                NodeKind::MidiInput
                | NodeKind::MidiOutput
                | NodeKind::MidiTranspose { .. }
                | NodeKind::MidiTransposeParameter { .. } => {
                    return Err(fail("MIDI nodes are only valid in midi_effect graphs"));
                }
                NodeKind::Gain { parameter } | NodeKind::Parameter { parameter } => {
                    if !self.parameters.iter().any(|p| p.id == *parameter) {
                        return Err(fail(&format!("unknown parameter {parameter:?}")));
                    }
                }
                NodeKind::Constant { value } if !value.is_finite() || value.abs() > f32::MAX as f64 => {
                    return Err(fail("constant must be finite and within f32 range"));
                }
                NodeKind::Delay { max_seconds }
                    if !max_seconds.is_finite() || *max_seconds <= 0.0 || *max_seconds > 60.0 =>
                {
                    return Err(fail("max_seconds must be greater than zero and at most 60"));
                }
                _ => {}
            }
        }
        if inputs != usize::from(!instrument) || outputs != 1 {
            return Err(DeviceSdkError(if midi {
                "exactly one midi_input and midi_output are required".into()
            } else if instrument {
                "instrument graphs have no audio_input and require exactly one audio_output".into()
            } else {
                "exactly one audio_input and audio_output are required".into()
            }));
        }
        if instrument && midi_sources == 0 {
            return Err(DeviceSdkError("an instrument graph needs at least one MIDI value node".into()));
        }
        if midi {
            let midi_input = self.nodes.iter().position(|n| n.kind == NodeKind::MidiInput);
            let midi_output = self.nodes.iter().position(|n| n.kind == NodeKind::MidiOutput);
            for (index, node) in self.nodes.iter().enumerate() {
                let incoming = self.connections.iter().filter(|edge| edge.to == node.id).count();
                let outgoing = self.connections.iter().filter(|edge| edge.from == node.id).count();
                let expected_in = usize::from(Some(index) != midi_input);
                let expected_out = usize::from(Some(index) != midi_output);
                if incoming != expected_in || outgoing != expected_out {
                    return Err(DeviceSdkError(format!(
                        "node {:?}: MIDI graph must be a single chain from midi_input to midi_output",
                        node.id
                    )));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn execution_order(&self) -> Result<Vec<usize>, DeviceSdkError> {
        let error = |message: String| DeviceSdkError(message);
        let mut edges = Vec::with_capacity(self.connections.len());
        let mut used = HashSet::new();
        for edge in &self.connections {
            let from = self
                .nodes
                .iter()
                .position(|n| n.id == edge.from)
                .ok_or_else(|| error(format!("connection from missing node {:?}", edge.from)))?;
            let to = self
                .nodes
                .iter()
                .position(|n| n.id == edge.to)
                .ok_or_else(|| error(format!("connection to missing node {:?}", edge.to)))?;
            let source = &self.nodes[from];
            let dest = &self.nodes[to];
            if matches!(source.kind, NodeKind::AudioOutput | NodeKind::MidiOutput) {
                return Err(error(format!("node {:?}: output cannot feed another node", source.id)));
            }
            if !source.kind.info().outputs.contains(&edge.from_port.as_str()) {
                return Err(error(format!("node {:?}: unknown output port {:?}", source.id, edge.from_port)));
            }
            if !dest.kind.info().inputs.iter().any(|p| p.name == edge.to_port) {
                return Err(error(format!("node {:?}: unknown input port {:?}", dest.id, edge.to_port)));
            }
            let target_resource = dest
                .kind
                .info()
                .inputs
                .iter()
                .find(|p| p.name == edge.to_port)
                .and_then(|p| matches!(p.unit, "sample" | "region" | "slice_map" | "midi").then_some(p.unit));
            if source.kind.output_resource(&edge.from_port) != target_resource {
                return Err(error(format!("incompatible resource ports from {:?} to {:?}", source.id, dest.id)));
            }
            if !used.insert((to, edge.to_port.as_str())) {
                return Err(error(format!(
                    "node {:?}, port {:?}: multiple connections; use add or mix",
                    dest.id, edge.to_port
                )));
            }
            edges.push((from, to, dest.kind.breaks_cycle(&edge.to_port)));
        }
        for (i, node) in self.nodes.iter().enumerate() {
            for port in node.kind.info().inputs {
                if port.default.is_none() && !used.contains(&(i, port.name)) {
                    return Err(error(format!("node {:?}: required port {:?} is not connected", node.id, port.name)));
                }
            }
        }
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut scheduled = vec![false; self.nodes.len()];
        loop {
            let next = (0..self.nodes.len()).find(|&i| {
                !scheduled[i] && edges.iter().all(|&(from, to, delayed)| to != i || delayed || scheduled[from])
            });
            let Some(i) = next else { break };
            scheduled[i] = true;
            order.push(i);
        }
        if order.len() != self.nodes.len() {
            let blocked = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, _)| !scheduled[*i])
                .map(|(_, n)| n.id.as_str())
                .collect::<Vec<_>>();
            return Err(error(format!("cycle without a delay input through nodes {blocked:?}")));
        }
        let output_kind =
            if self.role == DeviceRole::MidiEffect { NodeKind::MidiOutput } else { NodeKind::AudioOutput };
        let Some(output) = self.nodes.iter().position(|n| n.kind == output_kind) else {
            return Err(error("missing graph output".into()));
        };
        let mut reachable = vec![false; self.nodes.len()];
        reachable[output] = true;
        for _ in 0..self.nodes.len() {
            for &(from, to, _) in &edges {
                if reachable[to] {
                    reachable[from] = true;
                }
            }
        }
        if let Some((_, node)) = self.nodes.iter().enumerate().find(|(i, _)| !reachable[*i]) {
            return Err(error(format!("node {:?} does not contribute to audio_output", node.id)));
        }
        Ok(order)
    }

    pub(crate) fn delay_budget(&self, sample_rate: f64) -> Result<usize, DeviceSdkError> {
        let mut total = 0usize;
        for node in &self.nodes {
            let frames = match node.kind {
                NodeKind::SampleDelay => 2,
                NodeKind::Delay { max_seconds } => (max_seconds * sample_rate).ceil() as usize + 2,
                _ => 0,
            };
            total = total.checked_add(frames).ok_or_else(|| DeviceSdkError("delay size overflow".into()))?;
            if total > MAX_DELAY_FRAMES {
                return Err(DeviceSdkError(format!(
                    "node {:?}: combined delay history exceeds {MAX_DELAY_FRAMES} frames at {sample_rate} Hz; reduce max_seconds",
                    node.id
                )));
            }
        }
        Ok(total)
    }
}
