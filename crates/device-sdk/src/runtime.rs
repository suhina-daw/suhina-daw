// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A prepared block signal interpreter with per-frame feedback islands.
//! All persistent processing storage is allocated during compilation.
use crate::native_node::NativeNode;
use crate::{DeviceGraph, DeviceSdkError, FilterMode, NodeKind, Parameter, ProcessContext};
use suhina_dsp::{Biquad, Coefficients};

#[derive(Clone, Copy)]
struct Wire {
    source: usize,
    channel: Option<usize>,
    port: usize,
}
struct Op {
    id: String,
    kind: NodeKind,
    inputs: [Option<Wire>; 5],
    defaults: [f64; 5],
    parameter: Option<usize>,
    state: State,
    auxiliary_count: usize,
    event_source: Option<usize>,
    event_output: bool,
}
struct Island {
    nodes: Vec<usize>,
    sources: Vec<usize>,
}
enum State {
    Native(Box<dyn NativeNode>),
    None,
    Phase(f64),
    Random(u32),
    Smooth([f64; 2]),
    Held {
        value: [f64; 2],
        trigger: [f64; 2],
    },
    Filter {
        channels: [Biquad; 2],
        controls: [f64; 3],
    },
    Delay {
        history: Vec<[f64; 2]>,
        cursor: usize,
        max_frames: f64,
        heads: [f64; 2],
        targets: [f64; 2],
        fades: [usize; 2],
    },
}
/// Compiled graph. Prepare and drop off the audio thread. Processing, reset and parameter
/// updates neither allocate nor perform I/O. The same processor runs offline and live.
pub struct PreparedEffect {
    nodes: Vec<Op>,
    order: Vec<usize>,
    values: Vec<[f64; 2]>,
    auxiliary: Vec<Vec<[f64; 2]>>,
    block_auxiliary: Vec<Vec<[[f64; 2]; 64]>>,
    parameters: Vec<Parameter>,
    parameter_values: Vec<f64>,
    targets: Vec<f64>,
    remaining: Vec<usize>,
    started: bool,
    sample_rate: f64,
    output: usize,
    midi: [f64; 3],
    block_values: Vec<[[f64; 2]; 64]>,
    block_parameters: Vec<[f64; 64]>,
    block_safe: bool,
    islands: Vec<Island>,
    event_buffers: Vec<Vec<suhina_device_api::NoteEvent>>,
    event_counts: Vec<usize>,
    event_overflows: u64,
    binding_targets: Vec<Vec<(usize, String)>>,
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value.clamp(-(f32::MAX as f64), f32::MAX as f64) } else { 0.0 }
}
fn coefficient(seconds: f64, rate: f64) -> f64 {
    crate::modulation::coefficient(seconds, rate)
}

fn feedback_islands(nodes: &[Op], order: &[usize]) -> Vec<Island> {
    let count = nodes.len();
    let mut reachable = vec![vec![false; count]; count];
    for (target, op) in nodes.iter().enumerate() {
        reachable[target][target] = true;
        for wire in op.inputs.iter().flatten() {
            reachable[wire.source][target] = true;
        }
    }
    for via in 0..count {
        for from in 0..count {
            for to in 0..count {
                reachable[from][to] |= reachable[from][via] && reachable[via][to];
            }
        }
    }
    let mut scheduled = vec![false; count];
    let mut islands = Vec::new();
    for _ in 0..count {
        let Some(&root) = order.iter().find(|&&candidate| {
            !scheduled[candidate]
                && order
                    .iter()
                    .all(|&source| !reachable[source][candidate] || reachable[candidate][source] || scheduled[source])
        }) else {
            break;
        };
        let island: Vec<_> =
            order.iter().copied().filter(|&node| reachable[root][node] && reachable[node][root]).collect();
        for &node in &island {
            scheduled[node] = true;
        }
        let mut sources = Vec::new();
        for &node in &island {
            for wire in nodes[node].inputs.iter().flatten() {
                if !island.contains(&wire.source) && !sources.contains(&wire.source) {
                    sources.push(wire.source);
                }
            }
        }
        islands.push(Island { nodes: island, sources });
    }
    islands
}

impl PreparedEffect {
    pub(crate) fn new(graph: &DeviceGraph, sample_rate: f64) -> Result<Self, DeviceSdkError> {
        Self::new_with_native(graph, sample_rate, &mut |id, _| {
            Err(DeviceSdkError(format!("unregistered native effect {id:?}")))
        })
    }

    pub(crate) fn new_with_native(
        graph: &DeviceGraph,
        sample_rate: f64,
        resolve: &mut impl FnMut(&str, f64) -> Result<Box<dyn NativeNode>, DeviceSdkError>,
    ) -> Result<Self, DeviceSdkError> {
        crate::native_node::compile_graph(graph, sample_rate, None, resolve)
    }

    pub(crate) fn new_with_factory(
        graph: &DeviceGraph,
        sample_rate: f64,
        prepare: &mut impl FnMut(&NodeKind, f64) -> Result<Option<Box<dyn NativeNode>>, DeviceSdkError>,
    ) -> Result<Self, DeviceSdkError> {
        if !sample_rate.is_finite() || !(8_000.0..=384_000.0).contains(&sample_rate) {
            return Err(DeviceSdkError("sample rate must be finite and 8000..384000 Hz".into()));
        }
        graph.delay_budget(sample_rate)?;
        let order = graph.execution_order()?;
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for node in &graph.nodes {
            let mut inputs = [None; 5];
            let mut defaults = [0.0; 5];
            for (i, port) in node.kind.info().inputs.iter().enumerate() {
                let Some(default) = defaults.get_mut(i) else {
                    return Err(DeviceSdkError("too many node ports".into()));
                };
                *default = port.default.unwrap_or(0.0);
                if let Some(edge) =
                    graph.connections.iter().find(|edge| edge.to == node.id && edge.to_port == port.name)
                {
                    let source = graph
                        .nodes
                        .iter()
                        .position(|n| n.id == edge.from)
                        .ok_or_else(|| DeviceSdkError(format!("missing node {:?}", edge.from)))?;
                    let channel = match edge.from_port.as_str() {
                        "left" => Some(0),
                        "right" => Some(1),
                        _ => None,
                    };
                    if let Some(input) = inputs.get_mut(i) {
                        *input = Some(Wire {
                            source,
                            channel,
                            port: graph.nodes[source].kind.output_index(&edge.from_port),
                        });
                    }
                }
            }
            let parameter = match &node.kind {
                NodeKind::Gain { parameter } | NodeKind::Parameter { parameter } => {
                    graph.parameters.iter().position(|p| p.id == *parameter)
                }
                _ => None,
            };
            let prepared_native = prepare(&node.kind, sample_rate)?;
            let state = if let Some(mut native) = prepared_native {
                native.connected_inputs(inputs.map(|p| p.is_some()))?;
                for binding in &node.bindings {
                    if native.parameter_range(&binding.target).is_none() {
                        return Err(DeviceSdkError(format!("unknown native parameter {:?}", binding.target)));
                    }
                    if native
                        .parameter_input(&binding.target)
                        .is_some_and(|port| inputs.get(port).is_some_and(Option::is_some))
                    {
                        return Err(DeviceSdkError("a parameter cannot be bound and signal-driven at once".into()));
                    }
                    let value = graph
                        .parameters
                        .iter()
                        .find(|p| p.id == binding.parameter)
                        .ok_or_else(|| DeviceSdkError("unknown parent parameter".into()))?
                        .default;
                    native.set_parameter(&binding.target, value);
                }
                State::Native(native)
            } else {
                match node.kind {
                    NodeKind::Oscillator { .. } => State::Phase(0.0),
                    NodeKind::Noise { seed } => State::Random(seed.max(1)),
                    NodeKind::Smooth | NodeKind::Envelope { .. } => State::Smooth([0.0; 2]),
                    NodeKind::SampleHold => State::Held { value: [0.0; 2], trigger: [0.0; 2] },
                    NodeKind::Filter { .. } => {
                        State::Filter { channels: [Biquad::default(); 2], controls: [f64::NAN; 3] }
                    }
                    NodeKind::Delay { max_seconds } => State::Delay {
                        history: vec![[0.0; 2]; (max_seconds * sample_rate).ceil() as usize + 2],
                        cursor: 0,
                        max_frames: (max_seconds * sample_rate).max(1.0),
                        heads: [0.; 2],
                        targets: [0.; 2],
                        fades: [0; 2],
                    },
                    NodeKind::SampleDelay => State::Delay {
                        history: vec![[0.0; 2]; 2],
                        cursor: 0,
                        max_frames: 1.0,
                        heads: [0.; 2],
                        targets: [0.; 2],
                        fades: [0; 2],
                    },
                    _ => State::None,
                }
            };
            if !node.bindings.is_empty() && !matches!(state, State::Native(_)) {
                return Err(DeviceSdkError("parameter bindings require a native kernel".into()));
            }
            nodes.push(Op {
                id: node.id.clone(),
                kind: node.kind.clone(),
                inputs,
                defaults,
                parameter,
                state,
                auxiliary_count: node.kind.auxiliary_count(),
                event_source: graph
                    .connections
                    .iter()
                    .find(|edge| {
                        edge.to == node.id
                            && node.kind.info().inputs.iter().any(|p| p.name == edge.to_port && p.unit == "midi")
                    })
                    .and_then(|edge| graph.nodes.iter().position(|n| n.id == edge.from)),
                event_output: node.kind.resource_output() == Some("midi"),
            });
        }
        let output = graph
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::AudioOutput)
            .ok_or_else(|| DeviceSdkError("missing output".into()))?;
        let islands = feedback_islands(&nodes, &order);
        // When only I/O sits outside feedback, there is no block work to amortize the scratch copies.
        let block_safe = islands.iter().any(|island| {
            island.nodes.len() == 1
                && island
                    .nodes
                    .first()
                    .is_some_and(|&node| !matches!(nodes[node].kind, NodeKind::AudioInput | NodeKind::AudioOutput))
        });
        Ok(Self {
            nodes,
            order,
            output,
            sample_rate,
            values: vec![[0.0; 2]; graph.nodes.len()],
            event_buffers: graph
                .nodes
                .iter()
                .map(|n| {
                    if n.kind.resource_output() == Some("midi") {
                        vec![suhina_device_api::NoteEvent::AllNotesOff; 256]
                    } else {
                        vec![]
                    }
                })
                .collect(),
            event_counts: vec![0; graph.nodes.len()],
            event_overflows: 0,
            binding_targets: graph
                .parameters
                .iter()
                .map(|parameter| {
                    graph
                        .nodes
                        .iter()
                        .enumerate()
                        .flat_map(|(index, node)| {
                            node.bindings
                                .iter()
                                .filter(|b| b.parameter == parameter.id)
                                .map(move |binding| (index, binding.target.clone()))
                        })
                        .collect()
                })
                .collect(),
            auxiliary: graph.nodes.iter().map(|n| vec![[0.; 2]; n.kind.auxiliary_count()]).collect(),
            block_auxiliary: graph.nodes.iter().map(|n| vec![[[0.; 2]; 64]; n.kind.auxiliary_count()]).collect(),
            parameters: graph.parameters.clone(),
            parameter_values: graph.parameters.iter().map(|p| p.default).collect(),
            targets: graph.parameters.iter().map(|p| p.default).collect(),
            remaining: vec![0; graph.parameters.len()],
            started: false,
            midi: [0.0; 3],
            block_values: vec![[[0.0; 2]; 64]; graph.nodes.len()],
            block_parameters: vec![[0.0; 64]; graph.parameters.len()],
            block_safe,
            islands,
        })
    }

    /// Bind native resources off the audio thread; replacements may release storage.
    pub fn native_node_mut(&mut self, node: &str) -> Option<&mut (dyn NativeNode + 'static)> {
        self.nodes.iter_mut().find_map(|op| {
            if op.id != node {
                return None;
            }
            match &mut op.state {
                State::Native(native) => Some(native.as_mut()),
                _ => None,
            }
        })
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn set_parameter(&mut self, index: usize, value: f64) {
        if let Some(spec) = self.parameters.get(index) {
            let target = spec.clamp(value);
            let discrete = matches!(
                spec.control,
                crate::ParameterControl::Integer
                    | crate::ParameterControl::Toggle
                    | crate::ParameterControl::Button
                    | crate::ParameterControl::Choice
                    | crate::ParameterControl::Radio
            );
            if let Some(slot) = self.targets.get_mut(index) {
                if *slot == target {
                    return;
                }
                *slot = target;
            }
            if let Some(targets) = self.binding_targets.get(index) {
                for (node, target_id) in targets {
                    if let Some(Op { state: State::Native(native), .. }) = self.nodes.get_mut(*node) {
                        native.set_parameter(target_id, target);
                    }
                }
            }
            let frames = if self.started && !discrete {
                (spec.smoothing_ms * 0.001 * self.sample_rate).round() as usize
            } else {
                0
            };
            if let Some(slot) = self.remaining.get_mut(index) {
                *slot = frames;
            }
            if frames == 0
                && let Some(slot) = self.parameter_values.get_mut(index)
            {
                *slot = target;
            }
        }
    }

    pub(crate) fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.parameters.iter().find(|p| p.id == id).map(|p| (p.min, p.max))
    }
    pub(crate) fn set_named_parameter(&mut self, id: &str, value: f64) {
        if let Some(index) = self.parameters.iter().position(|p| p.id == id) {
            self.set_parameter(index, value);
        }
    }
    /// Clear DSP history without changing parameters or allocating.
    pub fn reset(&mut self) {
        self.values.fill([0.0; 2]);
        for outputs in &mut self.auxiliary {
            outputs.fill([0.; 2]);
        }
        self.midi = [0.0; 3];
        self.parameter_values.copy_from_slice(&self.targets);
        self.remaining.fill(0);
        self.started = false;
        for op in &mut self.nodes {
            match &mut op.state {
                State::Native(native) => native.reset(),
                State::Phase(phase) => *phase = 0.0,
                State::Random(value) => {
                    if let NodeKind::Noise { seed } = op.kind {
                        *value = seed.max(1);
                    }
                }
                State::Smooth(values) => values.fill(0.0),
                State::Held { value, trigger } => {
                    value.fill(0.0);
                    trigger.fill(0.0);
                }
                State::Filter { channels, controls } => {
                    for channel in channels {
                        channel.reset();
                    }
                    *controls = [f64::NAN; 3];
                }
                State::Delay { history, cursor, heads, targets, fades, .. } => {
                    heads.fill(0.);
                    targets.fill(0.);
                    fades.fill(0);
                    history.fill([0.0; 2]);
                    *cursor = 0;
                }
                State::None => {}
            }
        }
    }

    /// Process without a running transport. Oscillators and delay tails still advance.
    pub fn process(&mut self, buffer: &mut [f32]) {
        self.process_context(&ProcessContext::default(), buffer);
    }

    pub fn process_context(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        self.begin_span(context);
        self.process_prepared_context(context, buffer);
    }
    pub(crate) fn process_prepared_context(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        if self.block_safe {
            self.process_blocks(context, buffer);
            return;
        }
        let tempo = if context.tempo.is_finite() { context.tempo.clamp(1.0, 1000.0) } else { 120.0 };
        let mut beat = finite(context.beat);
        let beat_step = if context.playing { tempo / (60.0 * self.sample_rate) } else { 0.0 };
        let (chunks, remainder) = buffer.as_chunks_mut::<2>();
        for frame in chunks {
            self.started = true;
            for ((value, target), remaining) in
                self.parameter_values.iter_mut().zip(&self.targets).zip(&mut self.remaining)
            {
                if *remaining > 0 {
                    *value += (*target - *value) / *remaining as f64;
                    *remaining -= 1;
                    if *remaining == 0 {
                        *value = *target;
                    }
                }
            }
            let input = [finite(frame[0] as f64), finite(frame[1] as f64)];
            for &index in &self.order {
                evaluate(
                    &mut self.nodes,
                    &mut self.values,
                    &mut self.auxiliary,
                    &self.parameter_values,
                    self.midi,
                    self.sample_rate,
                    index,
                    input,
                    tempo,
                    beat,
                    context,
                );
            }
            // Commit delay inputs only after every node has evaluated the current frame.
            for op in &mut self.nodes {
                if let State::Delay { history, cursor, .. } = &mut op.state {
                    let input = op.inputs[0]
                        .and_then(|wire| {
                            (if wire.port == 0 {
                                self.values.get(wire.source)
                            } else {
                                self.auxiliary.get(wire.source).and_then(|ports| ports.get(wire.port - 1))
                            })
                            .map(|v| wire.channel.and_then(|c| v.get(c)).map_or(*v, |value| [*value; 2]))
                        })
                        .unwrap_or([0.0; 2]);
                    if let Some(slot) = history.get_mut(*cursor) {
                        *slot = input;
                    }
                    *cursor = (*cursor + 1) % history.len();
                }
            }
            let output = self.values.get(self.output).copied().unwrap_or([0.0; 2]);
            frame[0] = output[0] as f32;
            frame[1] = output[1] as f32;
            beat += beat_step;
        }
        remainder.fill(0.0);
    }

    fn process_blocks(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        let tempo = if context.tempo.is_finite() { context.tempo.clamp(1.0, 1000.0) } else { 120.0 };
        let mut beat = finite(context.beat);
        let step = if context.playing { tempo / (60.0 * self.sample_rate) } else { 0.0 };
        let (frames, remainder) = buffer.as_chunks_mut::<2>();
        for block in frames.chunks_mut(64) {
            let mut beats = [0.0; 64];
            for (frame, slot) in beats.iter_mut().take(block.len()).enumerate() {
                *slot = beat;
                beat += step;
                self.started = true;
                for (index, ((value, target), remaining)) in
                    self.parameter_values.iter_mut().zip(&self.targets).zip(&mut self.remaining).enumerate()
                {
                    if *remaining > 0 {
                        *value += (*target - *value) / *remaining as f64;
                        *remaining -= 1;
                        if *remaining == 0 {
                            *value = *target;
                        }
                    }
                    self.block_parameters[index][frame] = *value;
                }
            }
            for island in &self.islands {
                let Some(&index) = island.nodes.first() else { continue };
                if island.nodes.len() > 1 || matches!(self.nodes[index].state, State::Delay { .. }) {
                    for (frame, input) in block.iter().enumerate() {
                        for &source in &island.sources {
                            self.values[source] = self.block_values[source][frame];
                            for port in 0..self.nodes[source].auxiliary_count {
                                self.auxiliary[source][port] = self.block_auxiliary[source][port][frame];
                            }
                        }
                        for &node in &island.nodes {
                            if let Some(parameter) = self.nodes[node].parameter {
                                self.parameter_values[parameter] = self.block_parameters[parameter][frame];
                            }
                            evaluate(
                                &mut self.nodes,
                                &mut self.values,
                                &mut self.auxiliary,
                                &self.parameter_values,
                                self.midi,
                                self.sample_rate,
                                node,
                                input.map(|v| finite(f64::from(v))),
                                tempo,
                                beats[frame],
                                context,
                            );
                            self.block_values[node][frame] = self.values[node];
                            for port in 0..self.nodes[node].auxiliary_count {
                                self.block_auxiliary[node][port][frame] = self.auxiliary[node][port];
                            }
                        }
                        for &node in &island.nodes {
                            let op = &mut self.nodes[node];
                            if let State::Delay { history, cursor, .. } = &mut op.state {
                                let input = op.inputs[0]
                                    .and_then(|wire| {
                                        (if wire.port == 0 {
                                            self.values.get(wire.source)
                                        } else {
                                            self.auxiliary.get(wire.source).and_then(|ports| ports.get(wire.port - 1))
                                        })
                                        .map(|v| wire.channel.and_then(|c| v.get(c)).map_or(*v, |value| [*value; 2]))
                                    })
                                    .unwrap_or([0.0; 2]);
                                if let Some(slot) = history.get_mut(*cursor) {
                                    *slot = input;
                                }
                                *cursor = (*cursor + 1) % history.len();
                            }
                        }
                    }
                    continue;
                }
                let mut native_inputs = [[0.0; 128]; 5];
                if matches!(self.nodes[index].state, State::Native(_)) {
                    let op = &self.nodes[index];
                    for (port, samples) in native_inputs.iter_mut().enumerate() {
                        for (frame, pair) in samples.as_chunks_mut::<2>().0.iter_mut().take(block.len()).enumerate() {
                            let value = op.inputs[port].map_or([op.defaults[port]; 2], |wire| {
                                let value = if wire.port == 0 {
                                    self.block_values[wire.source][frame]
                                } else {
                                    self.block_auxiliary[wire.source][wire.port - 1][frame]
                                };
                                wire.channel.map_or(value, |channel| [value[channel]; 2])
                            });
                            *pair = value.map(|v| v as f32);
                        }
                    }
                }
                let auxiliary_count = self.nodes[index].auxiliary_count;
                if let State::Native(native) = &mut self.nodes[index].state {
                    let mut audio = [0.0; 128];
                    native.process(
                        native_inputs.each_ref().map(|v| &v[..block.len() * 2]),
                        &mut audio[..block.len() * 2],
                    );
                    for (frame, pair) in audio.as_chunks::<2>().0.iter().take(block.len()).enumerate() {
                        self.block_values[index][frame] = [finite(f64::from(pair[0])), finite(f64::from(pair[1]))];
                        for port in 0..auxiliary_count {
                            self.block_auxiliary[index][port][frame] =
                                native.output(port, frame).map(|v| finite(f64::from(v)));
                        }
                    }
                    continue;
                }
                // Select cheap kernels once per node, outside their sample loops.
                macro_rules! kernel {
                    ($render:expr) => {{
                        let op = &self.nodes[index];
                        for (frame, input) in block.iter().enumerate() {
                            let read = |port: usize| match op.inputs[port] {
                                Some(wire) => {
                                    let value = if wire.port == 0 {
                                        self.block_values[wire.source][frame]
                                    } else {
                                        self.block_auxiliary[wire.source][wire.port - 1][frame]
                                    };
                                    wire.channel.map_or(value, |channel| [value[channel]; 2])
                                }
                                None => [op.defaults[port]; 2],
                            };
                            let parameter = op.parameter.map_or(0.0, |p| self.block_parameters[p][frame]);
                            let output: [f64; 2] = $render(read(0), read(1), parameter, *input);
                            self.block_values[index][frame] = output.map(finite);
                        }
                        continue;
                    }};
                }
                match self.nodes[index].kind {
                    NodeKind::AudioInput => kernel!(|_, _, _, input: [f32; 2]| input.map(f64::from)),
                    NodeKind::AudioOutput => kernel!(|a, _, _, _| a),
                    NodeKind::Constant { value } => kernel!(|_, _, _, _| [value; 2]),
                    NodeKind::Parameter { .. } => kernel!(|_, _, p, _| [p; 2]),
                    NodeKind::Gain { .. } => kernel!(|a: [f64; 2], _, p, _| a.map(|v| v * p)),
                    NodeKind::Add => kernel!(|a: [f64; 2], b: [f64; 2], _, _| [a[0] + b[0], a[1] + b[1]]),
                    NodeKind::Multiply => kernel!(|a: [f64; 2], b: [f64; 2], _, _| [a[0] * b[0], a[1] * b[1]]),
                    _ => {}
                }
                for (frame, input) in block.iter().enumerate() {
                    for wire in self.nodes[index].inputs.iter().flatten() {
                        if wire.port == 0 {
                            self.values[wire.source] = self.block_values[wire.source][frame];
                        } else {
                            self.auxiliary[wire.source][wire.port - 1] =
                                self.block_auxiliary[wire.source][wire.port - 1][frame];
                        }
                    }
                    if let Some(parameter) = self.nodes[index].parameter {
                        self.parameter_values[parameter] = self.block_parameters[parameter][frame];
                    }
                    evaluate(
                        &mut self.nodes,
                        &mut self.values,
                        &mut self.auxiliary,
                        &self.parameter_values,
                        self.midi,
                        self.sample_rate,
                        index,
                        input.map(|v| finite(f64::from(v))),
                        tempo,
                        beats[frame],
                        context,
                    );
                    self.block_values[index][frame] = self.values[index];
                    for port in 0..self.nodes[index].auxiliary_count {
                        self.block_auxiliary[index][port][frame] = self.auxiliary[index][port];
                    }
                }
            }
            for (frame, output) in block.iter_mut().enumerate() {
                *output = self.block_values[self.output][frame].map(|v| v as f32);
            }
        }
        remainder.fill(0.0);
    }

    pub(crate) fn apply_event(&mut self, event: suhina_device_api::NoteEvent) {
        self.dispatch_event(event);
        match event {
            suhina_device_api::NoteEvent::On { key, velocity }
                if key <= 127 && velocity.is_finite() && velocity > 0.0 =>
            {
                self.midi = [f64::from(key), f64::from(velocity.clamp(0.0, 1.0)), 1.0];
            }
            suhina_device_api::NoteEvent::On { .. } => self.midi[2] = 0.0,
            suhina_device_api::NoteEvent::Off { key, .. } if f64::from(key) == self.midi[0] => self.midi[2] = 0.0,
            suhina_device_api::NoteEvent::AllNotesOff => self.midi[2] = 0.0,
            _ => {}
        }
    }
    pub(crate) fn set_external_inputs(&mut self, inputs: [&[f32]; 5]) {
        for node in &mut self.nodes {
            if let State::Native(native) = &mut node.state {
                native.set_external_inputs(inputs);
            }
        }
    }
    pub(crate) fn begin_span(&mut self, context: &ProcessContext) {
        for node in &mut self.nodes {
            if let State::Native(native) = &mut node.state {
                native.prepare_block(context);
            }
        }
    }

    pub fn event_overflows(&self) -> u64 {
        self.event_overflows
    }

    pub fn voice_finished(&self) -> Option<bool> {
        let mut seen = false;
        let mut finished = true;
        for node in &self.nodes {
            if let State::Native(native) = &node.state
                && let Some(done) = native.voice_finished()
            {
                seen = true;
                finished &= done;
            }
        }
        seen.then_some(finished)
    }

    pub(crate) fn dispatch_event(&mut self, event: suhina_device_api::NoteEvent) {
        self.event_counts.fill(0);
        let mut routed = [suhina_device_api::NoteEvent::AllNotesOff; 256];
        for &index in &self.order {
            let source = self.nodes[index].event_source;
            let count = source.map_or(1, |s| self.event_counts[s]);
            for at in 0..count {
                let incoming = source.map_or(event, |s| self.event_buffers[s][at]);
                let produced = if let State::Native(native) = &mut self.nodes[index].state {
                    native.route_event(incoming, &mut routed).min(routed.len())
                } else {
                    routed[0] = incoming;
                    1
                };
                if self.nodes[index].event_output {
                    let offset = self.event_counts[index];
                    let copied = produced.min(self.event_buffers[index].len().saturating_sub(offset));
                    self.event_buffers[index][offset..offset + copied].copy_from_slice(&routed[..copied]);
                    self.event_counts[index] += copied;
                    if copied < produced {
                        self.event_overflows = self.event_overflows.saturating_add(1);
                    }
                }
            }
        }
    }

    /// Render an instrument graph from timestamped MIDI events into its audio buffer.
    pub fn process_instrument_context(
        &mut self,
        context: &ProcessContext,
        events: &[(usize, suhina_device_api::NoteEvent)],
        buffer: &mut [f32],
    ) {
        let frames = buffer.len() / 2;
        let mut offset = 0;
        for &(frame, event) in events {
            let at = frame.min(frames).max(offset);
            if let Some(span) = buffer.get_mut(offset * 2..at * 2) {
                let mut span_context = *context;
                if span_context.playing {
                    span_context.beat += offset as f64 * span_context.tempo / (60.0 * self.sample_rate);
                }
                self.process_context(&span_context, span);
            }
            self.apply_event(event);
            offset = at;
        }
        if let Some(span) = buffer.get_mut(offset * 2..frames * 2) {
            let mut span_context = *context;
            if span_context.playing {
                span_context.beat += offset as f64 * span_context.tempo / (60.0 * self.sample_rate);
            }
            self.process_context(&span_context, span);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate(
    nodes: &mut [Op],
    values: &mut [[f64; 2]],
    auxiliary: &mut [Vec<[f64; 2]>],
    parameter_values: &[f64],
    midi: [f64; 3],
    sample_rate: f64,
    index: usize,
    input: [f64; 2],
    tempo: f64,
    beat: f64,
    context: &ProcessContext,
) {
    let Some(op) = nodes.get_mut(index) else { return };
    let x = std::array::from_fn::<_, 5, _>(|i| match op.inputs[i] {
        Some(Wire { source, channel, port }) => {
            let value = if port == 0 {
                values.get(source).copied()
            } else {
                auxiliary.get(source).and_then(|ports| ports.get(port - 1)).copied()
            }
            .unwrap_or([0.; 2]);
            channel.and_then(|i| value.get(i).copied()).map_or(value, |v| [v; 2])
        }
        None => [op.defaults[i]; 2],
    });
    let parameter = op.parameter.and_then(|i| parameter_values.get(i)).copied().unwrap_or(0.0);
    let mut output = [0.0; 2];
    use NodeKind::*;
    match (&op.kind, &mut op.state) {
        (_, State::Native(native)) => {
            let mut audio = [0.0; 2];
            let inputs = x.map(|pair| pair.map(|v| v as f32));
            native.process(inputs.each_ref().map(|v| v.as_slice()), &mut audio);
            output = audio.map(f64::from);
            for port in 0..op.auxiliary_count {
                if let Some(slot) = auxiliary.get_mut(index).and_then(|ports| ports.get_mut(port)) {
                    *slot = native.output(port, 0).map(|v| finite(f64::from(v)));
                }
            }
        }
        (AudioInput, _) => output = input,
        (AudioOutput, _) => output = x[0],
        (Constant { value }, _) => output = [*value; 2],
        (Parameter { .. }, _) => output = [parameter; 2],
        (Tempo, _) => output = [tempo; 2],
        (LfoRate, _) => {
            let hz = crate::modulation::lfo_hz(x[0][0], x[1][0], x[2][0], x[3][0], x[4][0]);
            output = [hz; 2];
        }
        (Beat, _) => output = [beat; 2],
        (Playing, _) => output = [f64::from(context.playing); 2],
        (MidiKey, _) => output = [midi[0]; 2],
        (MidiVelocity, _) => output = [midi[1]; 2],
        (MidiGate, _) => output = [midi[2]; 2],
        (Noise { .. }, State::Random(seed)) => {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 17;
            *seed ^= *seed << 5;
            output = [*seed as f64 / u32::MAX as f64 * 2.0 - 1.0; 2];
        }
        (Oscillator { shape }, State::Phase(phase)) => {
            output = [suhina_dsp::lfo::wave(shape.dsp(), *phase); 2];
            *phase = (*phase + x[0][0].clamp(-0.49 * sample_rate, 0.49 * sample_rate) / sample_rate).rem_euclid(1e9);
        }
        (SyncedLfo { shape }, _) => output = [suhina_dsp::lfo::wave(shape.dsp(), beat * x[0][0]); 2],
        (Stereo, _) => output = [x[0][0], x[1][1]],
        (Mono, _) => output = [(x[0][0] + x[0][1]) * 0.5; 2],
        (MidSideEncode, _) => output = [(x[0][0] + x[0][1]) * 0.5, (x[0][0] - x[0][1]) * 0.5],
        (MidSideDecode, _) => output = [x[0][0] + x[0][1], x[0][0] - x[0][1]],
        (Pan, _) => output = [x[0][0] * (1.0 - x[1][0].clamp(0.0, 1.0)), x[0][1] * (1.0 + x[1][0].clamp(-1.0, 0.0))],
        (Filter { mode }, State::Filter { channels, controls }) => {
            let next =
                [x[1][0].clamp(1.0, sample_rate * 0.49), x[2][0].clamp(0.025, 100.0), x[3][0].clamp(-48.0, 48.0)];
            if next != *controls {
                *controls = next;
                let [f, q, g] = next;
                let c = match mode {
                    FilterMode::Lowpass => Coefficients::lowpass(sample_rate, f, q),
                    FilterMode::Highpass => Coefficients::highpass(sample_rate, f, q),
                    FilterMode::Bandpass => Coefficients::bandpass(sample_rate, f, q),
                    FilterMode::Peak => Coefficients::peak(sample_rate, f, q, g),
                    FilterMode::LowShelf => Coefficients::low_shelf(sample_rate, f, q, g),
                    FilterMode::HighShelf => Coefficients::high_shelf(sample_rate, f, q, g),
                };
                for channel in channels.iter_mut() {
                    channel.set(c);
                }
            }
            for c in 0..2 {
                output[c] = channels[c].process(x[0][c]);
            }
        }
        (Delay { .. } | SampleDelay, State::Delay { history, cursor, max_frames, heads, targets, fades }) => {
            let len = history.len();
            let fade_frames = (sample_rate * 0.02).max(1.0) as usize;
            for c in 0..2 {
                let frames =
                    if matches!(op.kind, SampleDelay) { 1.0 } else { (x[1][c] * sample_rate).clamp(1.0, *max_frames) };
                let read = |distance: f64| {
                    let whole = distance.floor() as usize;
                    let fraction = distance - whole as f64;
                    let at = (*cursor + len - whole) % len;
                    let prev = (at + len - 1) % len;
                    history
                        .get(at)
                        .zip(history.get(prev))
                        .map_or(0.0, |(a, b)| a[c] * (1.0 - fraction) + b[c] * fraction)
                };
                if x[2][0].round() == 1.0 && heads[c] > 0.0 {
                    if fades[c] == 0 && (frames - targets[c]).abs() >= 1.0 {
                        heads[c] = targets[c];
                        targets[c] = frames;
                        fades[c] = fade_frames;
                    }
                    if fades[c] > 0 {
                        let blend = 1.0 - fades[c] as f64 / fade_frames as f64;
                        output[c] = read(heads[c]) * (1.0 - blend) + read(targets[c]) * blend;
                        fades[c] -= 1;
                        if fades[c] == 0 {
                            heads[c] = targets[c];
                        }
                    } else {
                        output[c] = read(targets[c]);
                    }
                } else {
                    let frames = if x[2][0].round() == 0.0 && heads[c] > 0.0 {
                        heads[c] + (frames - heads[c]) / fade_frames as f64
                    } else {
                        frames
                    };
                    output[c] = read(frames);
                    heads[c] = frames;
                    targets[c] = frames;
                    fades[c] = 0;
                }
            }
        }
        (Smooth | Envelope { .. }, State::Smooth(state)) => {
            for c in 0..2 {
                let rms = matches!(op.kind, Envelope { rms: true });
                let target = if matches!(op.kind, Smooth) {
                    x[0][c]
                } else if rms {
                    x[0][c] * x[0][c]
                } else {
                    x[0][c].abs()
                };
                let seconds = if matches!(op.kind, Smooth) || target > state[c] { x[1][c] } else { x[2][c] };
                let a = coefficient(seconds, sample_rate);
                state[c] = finite(a * state[c] + (1.0 - a) * target);
                output[c] = if rms { state[c].max(0.0).sqrt() } else { state[c] };
            }
        }
        (SampleHold, State::Held { value, trigger }) => {
            for c in 0..2 {
                if x[1][c] > 0.0 && trigger[c] <= 0.0 {
                    value[c] = x[0][c];
                }
                trigger[c] = x[1][c];
            }
            output = *value;
        }
        (kind, _) => {
            for c in 0..2 {
                let a = x[0][c];
                let b = x[1][c];
                let d = x[2][c];
                output[c] = match kind {
                    Gain { .. } => a * parameter,
                    Add => a + b,
                    Subtract => a - b,
                    Multiply => a * b,
                    Divide => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                    Min => a.min(b),
                    Max => a.max(b),
                    Power => a.powf(b),
                    Less => f64::from(a < b),
                    Greater => f64::from(a > b),
                    Abs => a.abs(),
                    Negate => -a,
                    Sqrt => a.max(0.0).sqrt(),
                    Exp => a.exp(),
                    Log => {
                        if a > 0.0 {
                            a.ln()
                        } else {
                            0.0
                        }
                    }
                    Floor => a.floor(),
                    Fract => a - a.floor(),
                    Sin => a.sin(),
                    Cos => a.cos(),
                    Tanh => a.tanh(),
                    Atan => a.atan(),
                    DbToGain => 10.0f64.powf(a.clamp(-120.0, 120.0) / 20.0),
                    GainToDb => (20.0 * a.abs().max(1e-6).log10()).max(-120.0),
                    Clamp => a.clamp(b.min(d), b.max(d)),
                    Mix => a * (1.0 - d.clamp(0.0, 1.0)) + b * d.clamp(0.0, 1.0),
                    Select => {
                        if d > 0.0 {
                            b
                        } else {
                            a
                        }
                    }
                    _ => 0.0,
                };
            }
        }
    }
    if let Some(slot) = values.get_mut(index) {
        *slot = output.map(finite);
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod block_tests;
