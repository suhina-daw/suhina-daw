// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A prepared per-frame signal interpreter. All storage is allocated during compilation.
use crate::{DeviceGraph, DeviceSdkError, FilterMode, NodeKind, Parameter, ProcessContext};
use suhina_dsp::{Biquad, Coefficients};

#[derive(Clone, Copy)]
struct Wire {
    source: usize,
    channel: Option<usize>,
}
struct Op {
    kind: NodeKind,
    inputs: [Option<Wire>; 5],
    defaults: [f64; 5],
    parameter: Option<usize>,
    state: State,
}
enum State {
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
    parameters: Vec<Parameter>,
    parameter_values: Vec<f64>,
    targets: Vec<f64>,
    remaining: Vec<usize>,
    started: bool,
    sample_rate: f64,
    output: usize,
    midi: [f64; 3],
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value.clamp(-(f32::MAX as f64), f32::MAX as f64) } else { 0.0 }
}
fn coefficient(seconds: f64, rate: f64) -> f64 {
    if seconds <= 0.0 { 0.0 } else { (-1.0 / (seconds.min(60.0) * rate)).exp() }
}

impl PreparedEffect {
    pub(crate) fn new(graph: &DeviceGraph, sample_rate: f64) -> Result<Self, DeviceSdkError> {
        if graph.nodes.iter().any(|n| matches!(n.kind, NodeKind::NativeDevice { .. })) {
            return Err(DeviceSdkError(
                "native_device requires prepare_native_at and a registered host factory".into(),
            ));
        }
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
                        *input = Some(Wire { source, channel });
                    }
                }
            }
            let parameter = match &node.kind {
                NodeKind::Gain { parameter } | NodeKind::Parameter { parameter } => {
                    graph.parameters.iter().position(|p| p.id == *parameter)
                }
                _ => None,
            };
            let state = match node.kind {
                NodeKind::Oscillator { .. } => State::Phase(0.0),
                NodeKind::Noise { seed } => State::Random(seed.max(1)),
                NodeKind::Smooth | NodeKind::Envelope { .. } => State::Smooth([0.0; 2]),
                NodeKind::SampleHold => State::Held { value: [0.0; 2], trigger: [0.0; 2] },
                NodeKind::Filter { .. } => State::Filter { channels: [Biquad::default(); 2], controls: [f64::NAN; 3] },
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
            };
            nodes.push(Op { kind: node.kind.clone(), inputs, defaults, parameter, state });
        }
        let output = graph
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::AudioOutput)
            .ok_or_else(|| DeviceSdkError("missing output".into()))?;
        Ok(Self {
            nodes,
            order,
            output,
            sample_rate,
            values: vec![[0.0; 2]; graph.nodes.len()],
            parameters: graph.parameters.clone(),
            parameter_values: graph.parameters.iter().map(|p| p.default).collect(),
            targets: graph.parameters.iter().map(|p| p.default).collect(),
            remaining: vec![0; graph.parameters.len()],
            started: false,
            midi: [0.0; 3],
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

    /// Clear DSP history without changing parameters or allocating.
    pub fn reset(&mut self) {
        self.values.fill([0.0; 2]);
        self.midi = [0.0; 3];
        self.parameter_values.copy_from_slice(&self.targets);
        self.remaining.fill(0);
        self.started = false;
        for op in &mut self.nodes {
            match &mut op.state {
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
                let Some(op) = self.nodes.get_mut(index) else { continue };
                let x = std::array::from_fn::<_, 5, _>(|i| match op.inputs[i] {
                    Some(Wire { source, channel }) => {
                        let value = self.values.get(source).copied().unwrap_or([0.0; 2]);
                        channel.and_then(|i| value.get(i).copied()).map_or(value, |v| [v; 2])
                    }
                    None => [op.defaults[i]; 2],
                });
                let parameter = op.parameter.and_then(|i| self.parameter_values.get(i)).copied().unwrap_or(0.0);
                let mut output = [0.0; 2];
                use NodeKind::*;
                match (&op.kind, &mut op.state) {
                    (AudioInput, _) => output = input,
                    (AudioOutput, _) => output = x[0],
                    (Constant { value }, _) => output = [*value; 2],
                    (Parameter { .. }, _) => output = [parameter; 2],
                    (Tempo, _) => output = [tempo; 2],
                    (LfoRate, _) => {
                        let hz = crate::sampler::lfo_hz(x[0][0], x[1][0], x[2][0], x[3][0], x[4][0]);
                        output = [hz; 2];
                    }
                    (Beat, _) => output = [beat; 2],
                    (Playing, _) => output = [f64::from(context.playing); 2],
                    (MidiKey, _) => output = [self.midi[0]; 2],
                    (MidiVelocity, _) => output = [self.midi[1]; 2],
                    (MidiGate, _) => output = [self.midi[2]; 2],
                    (Noise { .. }, State::Random(seed)) => {
                        *seed ^= *seed << 13;
                        *seed ^= *seed >> 17;
                        *seed ^= *seed << 5;
                        output = [*seed as f64 / u32::MAX as f64 * 2.0 - 1.0; 2];
                    }
                    (Oscillator { shape }, State::Phase(phase)) => {
                        output = [suhina_dsp::lfo::wave(shape.dsp(), *phase); 2];
                        *phase = (*phase
                            + x[0][0].clamp(-0.49 * self.sample_rate, 0.49 * self.sample_rate) / self.sample_rate)
                            .rem_euclid(1e9);
                    }
                    (SyncedLfo { shape }, _) => output = [suhina_dsp::lfo::wave(shape.dsp(), beat * x[0][0]); 2],
                    (Stereo, _) => output = [x[0][0], x[1][1]],
                    (Mono, _) => output = [(x[0][0] + x[0][1]) * 0.5; 2],
                    (MidSideEncode, _) => output = [(x[0][0] + x[0][1]) * 0.5, (x[0][0] - x[0][1]) * 0.5],
                    (MidSideDecode, _) => output = [x[0][0] + x[0][1], x[0][0] - x[0][1]],
                    (Pan, _) => {
                        output = [x[0][0] * (1.0 - x[1][0].clamp(0.0, 1.0)), x[0][1] * (1.0 + x[1][0].clamp(-1.0, 0.0))]
                    }
                    (Filter { mode }, State::Filter { channels, controls }) => {
                        let next = [
                            x[1][0].clamp(1.0, self.sample_rate * 0.49),
                            x[2][0].clamp(0.025, 100.0),
                            x[3][0].clamp(-48.0, 48.0),
                        ];
                        if next != *controls {
                            *controls = next;
                            let [f, q, g] = next;
                            let c = match mode {
                                FilterMode::Lowpass => Coefficients::lowpass(self.sample_rate, f, q),
                                FilterMode::Highpass => Coefficients::highpass(self.sample_rate, f, q),
                                FilterMode::Bandpass => Coefficients::bandpass(self.sample_rate, f, q),
                                FilterMode::Peak => Coefficients::peak(self.sample_rate, f, q, g),
                                FilterMode::LowShelf => Coefficients::low_shelf(self.sample_rate, f, q, g),
                                FilterMode::HighShelf => Coefficients::high_shelf(self.sample_rate, f, q, g),
                            };
                            for channel in channels.iter_mut() {
                                channel.set(c);
                            }
                        }
                        for c in 0..2 {
                            output[c] = channels[c].process(x[0][c]);
                        }
                    }
                    (
                        Delay { .. } | SampleDelay,
                        State::Delay { history, cursor, max_frames, heads, targets, fades },
                    ) => {
                        let len = history.len();
                        let fade_frames = (self.sample_rate * 0.02).max(1.0) as usize;
                        for c in 0..2 {
                            let frames = if matches!(op.kind, SampleDelay) {
                                1.0
                            } else {
                                (x[1][c] * self.sample_rate).clamp(1.0, *max_frames)
                            };
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
                            let seconds =
                                if matches!(op.kind, Smooth) || target > state[c] { x[1][c] } else { x[2][c] };
                            let a = coefficient(seconds, self.sample_rate);
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
                if let Some(slot) = self.values.get_mut(index) {
                    *slot = output.map(finite);
                }
            }
            // Commit delay inputs only after every node has evaluated the current frame.
            for op in &mut self.nodes {
                if let State::Delay { history, cursor, .. } = &mut op.state {
                    let input = op.inputs[0]
                        .and_then(|wire| {
                            self.values
                                .get(wire.source)
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
