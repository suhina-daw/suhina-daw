// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub(crate) const GRAIN_WINDOW: NodeInfo = NodeInfo {
    kind: "grain_window",
    name: "GrainWindow",
    rust_variant: "GrainWindow",
    inputs: &[
        PortInfo { name: "position", default: Some(0.0), unit: "signal" },
        PortInfo { name: "duration", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Hann grain lifetime window, zero outside duration.",
    configuration: "none",
    example: r#"{"kind":"grain_window"}"#,
};
fn grain_window(x: [f64; 5]) -> f64 {
    if x[1] > 0. && x[0] >= 0. && x[0] < x[1] { hann(x[0] / x[1]) } else { 0. }
}

pub(crate) fn prepare(kind: &super::NodeKind) -> Option<Box<dyn crate::native_node::NativeNode>> {
    match kind {
        super::NodeKind::GrainWindow => Some(super::processor::boxed(grain_window)),
        _ => None,
    }
}

pub(crate) fn hann(phase: f64) -> f64 {
    0.5 - 0.5 * (std::f64::consts::TAU * phase).cos()
}

use crate::{
    Connection as C, DeviceGraph, DeviceRole, DeviceSdkError, Node as N, NodeKind, Parameter, PreparedEffect,
    ProcessContext, native_node::NativeNode,
};
use suhina_device_api::NoteEvent;
pub(crate) const GRAIN_SCHEDULER: NodeInfo = NodeInfo {
    kind: "grain_scheduler",
    name: "GrainScheduler",
    rust_variant: "GrainScheduler",
    inputs: &[
        PortInfo { name: "density", default: Some(8.), unit: "Hz" },
        PortInfo { name: "gate", default: Some(1.), unit: "signal" },
        PortInfo { name: "trigger", default: Some(0.), unit: "signal" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Bounded periodic grain starts with event-timed retrigger.",
    configuration: "none",
    example: r#"{"kind":"grain_scheduler"}"#,
};
pub(crate) fn tick(remaining: &mut f64, interval: f64) -> bool {
    *remaining -= 1.;
    if *remaining <= 0. {
        *remaining += interval.max(1.);
        true
    } else {
        false
    }
}
struct Scheduler {
    rate: f64,
    remaining: [f64; 2],
    gate: [bool; 2],
    trigger: [bool; 2],
    note: bool,
}
pub(crate) fn prepare_scheduler(kind: &NodeKind, rate: f64) -> Option<Box<dyn NativeNode>> {
    matches!(kind, NodeKind::GrainScheduler).then(|| {
        Box::new(Scheduler { rate, remaining: [0.; 2], gate: [false; 2], trigger: [false; 2], note: false })
            as Box<dyn NativeNode>
    })
}
impl NativeNode for Scheduler {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            for (c, sample) in pair.iter_mut().enumerate() {
                let x: [f64; 3] = std::array::from_fn(|p| {
                    let v = inputs[p].get(frame * 2 + c).copied().unwrap_or(0.);
                    if v.is_finite() { f64::from(v) } else { 0. }
                });
                let gate = x[1] > 0.;
                let trigger = x[2] > 0.;
                let fire = self.note || (gate && !self.gate[c]) || (trigger && !self.trigger[c]);
                self.gate[c] = gate;
                self.trigger[c] = trigger;
                let density = x[0].clamp(0., self.rate);
                *sample = if fire {
                    self.remaining[c] = 0.;
                    if density > 0. {
                        let _ = tick(&mut self.remaining[c], self.rate / density);
                    }
                    1.
                } else if gate && density > 0. && tick(&mut self.remaining[c], self.rate / density) {
                    1.
                } else {
                    0.
                };
            }
            self.note = false;
        }
    }
    fn event(&mut self, event: NoteEvent) {
        if matches!(super::events::normalized(event), Some(NoteEvent::On { .. })) {
            self.note = true;
        }
    }
    fn reset(&mut self) {
        self.remaining = [0.; 2];
        self.gate = [false; 2];
        self.trigger = [false; 2];
        self.note = false;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
pub(crate) const GRAIN_POOL: NodeInfo = NodeInfo {
    kind: "grain_pool",
    name: "GrainPool",
    rust_variant: "GrainPool",
    inputs: &[
        PortInfo { name: "trigger", default: Some(0.0), unit: "signal" },
        PortInfo { name: "position", default: Some(0.0), unit: "frames" },
        PortInfo { name: "rate", default: Some(1.0), unit: "frames/sample" },
        PortInfo { name: "duration", default: Some(0.12), unit: "seconds" },
        PortInfo { name: "pan", default: Some(0.0), unit: "balance" },
    ],
    outputs: &["out", "left", "right", "finished", "dropped"],
    description: "Prepared private grain graphs started by a rising trigger, bounded lifetime and drop-new overflow.",
    configuration: "grains 1..128 and embedded instrument graph",
    example: r#"{"kind":"grain_pool","grains":8,"graph":"{\"format\":\"suhina-device\",\"version\":4,\"device\":{\"id\":\"org.suhina.grain-example\",\"name\":\"Grain\",\"role\":\"instrument\",\"parameters\":[]},\"nodes\":[{\"id\":\"voice\",\"kind\":\"voice_input\",\"controller\":1},{\"id\":\"done\",\"kind\":\"constant\",\"value\":0.0},{\"id\":\"result\",\"kind\":\"voice_output\"},{\"id\":\"out\",\"kind\":\"audio_output\"}],\"connections\":[{\"from\":\"voice\",\"to\":\"result\"},{\"from\":\"done\",\"to\":\"result\",\"to_port\":\"finished\"},{\"from\":\"result\",\"to\":\"out\"}]}"}"#,
};
pub(crate) const GRANULAR_PLAYER: NodeInfo = NodeInfo {
    kind: "granular_player",
    name: "GranularPlayer",
    rust_variant: "GranularPlayer",
    inputs: &[
        PortInfo { name: "position", default: Some(0.5), unit: "normalized" },
        PortInfo { name: "size", default: Some(0.12), unit: "seconds" },
        PortInfo { name: "density", default: Some(8.), unit: "Hz" },
        PortInfo { name: "pitch", default: Some(1.), unit: "ratio" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Granular instrument compiled from the same editable grain and voice graphs.",
    configuration: "sample, voices, grains and seed",
    example: r#"{"kind":"granular_player","sample":"sample","voices":8,"grains":16,"seed":1}"#,
};
struct Grain {
    graph: PreparedEffect,
    active: bool,
    position: f32,
    rate: f32,
    duration: f32,
    pan: f32,
    age: u64,
}
struct Pool {
    grains: Vec<Grain>,
    context: ProcessContext,
    rate: f64,
    dropped: u64,
    key: u8,
    velocity: f32,
    controllers: [f32; 128],
    bend: f32,
    pressure: f32,
    cache: [[[f32; 2]; 64]; 2],
    /// Whether the trigger was above zero on the frame before: a grain starts as it rises.
    triggered: bool,
}
pub(crate) fn pool(graphs: Vec<PreparedEffect>, rate: f64) -> Box<dyn NativeNode> {
    Box::new(Pool {
        grains: graphs
            .into_iter()
            .map(|graph| Grain { graph, active: false, position: 0., rate: 1., duration: 0., pan: 0., age: 0 })
            .collect(),
        context: ProcessContext::default(),
        rate,
        dropped: 0,
        key: 60,
        velocity: 1.,
        controllers: [0.; 128],
        bend: 0.,
        pressure: 0.,
        cache: [[[0.; 2]; 64]; 2],
        triggered: false,
    })
}
impl NativeNode for Pool {
    fn prepare_block(&mut self, value: &ProcessContext) {
        self.context = super::component::context(value);
        for grain in &mut self.grains {
            if grain.active {
                grain.graph.begin_span(&self.context);
            }
        }
    }
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let x: [f32; 5] = std::array::from_fn(|p| {
                let v = inputs[p].get(frame * 2).copied().unwrap_or(0.);
                if v.is_finite() { v } else { 0. }
            });
            // A rising trigger starts one grain; one held high (a gate) does not fill the pool.
            let rising = x[0] > 0. && !self.triggered;
            self.triggered = x[0] > 0.;
            if rising {
                if let Some(grain) = self.grains.iter_mut().find(|g| !g.active) {
                    grain.graph.reset();
                    grain.active = true;
                    grain.position = x[1];
                    grain.rate = x[2];
                    grain.duration = x[3].clamp(0., 60.);
                    grain.pan = x[4].clamp(-1., 1.);
                    grain.age = 0;
                    for (number, &value) in self.controllers.iter().enumerate() {
                        if value != 0. {
                            grain.graph.apply_event(NoteEvent::Controller { number: number as u8, value });
                        }
                    }
                    grain.graph.apply_event(NoteEvent::PitchBend(self.bend));
                    grain.graph.apply_event(NoteEvent::Pressure(self.pressure));
                    grain.graph.apply_event(NoteEvent::On { key: self.key, velocity: self.velocity });
                    grain.graph.begin_span(&self.context);
                } else {
                    self.dropped = self.dropped.saturating_add(1);
                }
            }
            let mut sum = [0f64; 2];
            for grain in &mut self.grains {
                if !grain.active {
                    continue;
                }
                if grain.age as f64 >= f64::from(grain.duration) * self.rate {
                    grain.active = false;
                    continue;
                }
                let controls = [
                    [grain.position; 2],
                    [grain.rate; 2],
                    [grain.duration; 2],
                    [grain.pan; 2],
                    [(grain.age as f64 / self.rate) as f32; 2],
                ];
                let mut audio = [0.; 2];
                grain.graph.set_external_inputs(controls.each_ref().map(|v| v.as_slice()));
                grain.graph.process_prepared_context(&self.context, &mut audio);
                for c in 0..2 {
                    sum[c] += f64::from(audio[c]);
                }
                grain.age = grain.age.saturating_add(1);
                if grain.graph.voice_finished() == Some(true) {
                    grain.active = false;
                }
            }
            *pair = sum.map(|v| v.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32);
            self.cache[0][frame] = [f32::from(!self.grains.iter().any(|g| g.active)); 2];
            self.cache[1][frame] = [self.dropped as f32; 2];
            super::component::advance(&mut self.context, 1, self.rate);
        }
    }
    fn output(&self, port: usize, frame: usize) -> [f32; 2] {
        self.cache.get(port).and_then(|p| p.get(frame)).copied().unwrap_or([0.; 2])
    }
    fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.grains.first().and_then(|v| v.graph.parameter_range(id))
    }
    fn set_parameter(&mut self, id: &str, value: f64) {
        for grain in &mut self.grains {
            grain.graph.set_named_parameter(id, value);
        }
    }
    fn event(&mut self, event: NoteEvent) {
        let Some(event) = super::events::normalized(event) else {
            return;
        };
        match event {
            NoteEvent::On { key, velocity } => {
                self.key = key;
                self.velocity = velocity;
            }
            NoteEvent::Controller { number, value } => {
                self.controllers[usize::from(number)] = value;
                for g in &mut self.grains {
                    if g.active {
                        g.graph.apply_event(event);
                    }
                }
            }
            NoteEvent::PitchBend(v) => {
                self.bend = v;
                for grain in &mut self.grains {
                    if grain.active {
                        grain.graph.apply_event(event);
                    }
                }
            }
            NoteEvent::Pressure(v) => {
                self.pressure = v;
                for grain in &mut self.grains {
                    if grain.active {
                        grain.graph.apply_event(event);
                    }
                }
            }
            NoteEvent::AllNotesOff => {
                for g in &mut self.grains {
                    g.active = false;
                    g.graph.reset();
                }
            }
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.dropped = 0;
        self.key = 60;
        self.velocity = 1.;
        self.controllers.fill(0.);
        self.bend = 0.;
        self.pressure = 0.;
        self.cache.fill([[0.; 2]; 64]);
        self.triggered = false;
        for g in &mut self.grains {
            g.active = false;
            g.age = 0;
            g.graph.reset();
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
fn c(id: &str, value: f64) -> N {
    N::new(id, NodeKind::Constant { value })
}
/// Editable private grain body: read, Hann window, pan and completion.
pub fn grain_template(sample: &str) -> Result<DeviceGraph, DeviceSdkError> {
    let mut g = DeviceGraph::new("org.suhina.grain-template", "Grain", DeviceRole::Instrument);
    g.nodes = vec![
        N::new("asset", NodeKind::SampleAsset { sample: sample.into() }),
        N::new("read", NodeKind::GrainRead),
        N::new("position", NodeKind::ComponentInput { index: 0 }),
        N::new("rate", NodeKind::ComponentInput { index: 1 }),
        N::new("duration", NodeKind::ComponentInput { index: 2 }),
        N::new("pan", NodeKind::ComponentInput { index: 3 }),
        N::new("window", NodeKind::GrainWindow),
        N::new("amp", NodeKind::Multiply),
        N::new("balance", NodeKind::Pan),
        N::new("voice_out", NodeKind::VoiceOutput),
        N::audio_output("out"),
    ];
    g.connections = vec![
        C::ports("asset", "out", "read", "asset"),
        C::ports("position", "out", "read", "position"),
        C::ports("rate", "out", "read", "rate"),
        C::ports("duration", "out", "read", "duration"),
        C::ports("read", "age", "window", "position"),
        C::ports("duration", "out", "window", "duration"),
        C::ports("read", "out", "amp", "a"),
        C::ports("window", "out", "amp", "b"),
        C::new("amp", "balance"),
        C::ports("pan", "out", "balance", "balance"),
        C::new("balance", "voice_out"),
        C::ports("read", "finished", "voice_out", "finished"),
        C::new("voice_out", "out"),
    ];
    g.validate()?;
    Ok(g)
}
/// Complete editable granular instrument with stable position/size/density/pitch controls.
pub fn template(sample: &str, voices: u8, grains: u16, seed: u32) -> Result<DeviceGraph, DeviceSdkError> {
    let mut body = DeviceGraph::new("org.suhina.granular-voice", "Granular voice", DeviceRole::Instrument);
    body.nodes = vec![
        N::new("voice", NodeKind::VoiceInput { controller: 1 }),
        N::new("asset", NodeKind::SampleAsset { sample: sample.into() }),
        N::new("frames", NodeKind::SampleInfo { field: crate::SampleInfoField::Frames }),
        N::new("source_rate", NodeKind::SampleInfo { field: crate::SampleInfoField::SampleRate }),
        N::new("output_rate", NodeKind::SampleRate),
        N::new("ratio", NodeKind::Divide),
        N::new("pitch_rate", NodeKind::PlaybackRate),
        c("bend_range", 2.),
        N::new("bend", NodeKind::Multiply),
        N::new("pitch", NodeKind::ComponentInput { index: 4 }),
        N::new("speed", NodeKind::Multiply),
        N::new("position", NodeKind::ComponentInput { index: 1 }),
        N::new("position_frames", NodeKind::Multiply),
        N::new("size", NodeKind::ComponentInput { index: 2 }),
        N::new("density", NodeKind::ComponentInput { index: 3 }),
        N::new("scheduler", NodeKind::GrainScheduler),
        c("spread_amount", 0.1),
        N::new("spread_frames", NodeKind::Multiply),
        N::new("jitter", NodeKind::RandomSpread { seed }),
        N::new("pool", NodeKind::GrainPool { graph: grain_template(sample)?.to_sd()?, grains }),
        N::new("amp", NodeKind::Multiply),
        c("gate_threshold", 0.5),
        N::new("released", NodeKind::Less),
        N::new("finished", NodeKind::Multiply),
        N::new("voice_out", NodeKind::VoiceOutput),
        N::audio_output("out"),
    ];
    body.connections = vec![
        C::ports("asset", "out", "frames", "asset"),
        C::ports("asset", "out", "source_rate", "asset"),
        C::ports("source_rate", "out", "ratio", "a"),
        C::ports("output_rate", "out", "ratio", "b"),
        C::ports("voice", "key", "pitch_rate", "key"),
        C::ports("voice", "bend", "bend", "a"),
        C::ports("bend_range", "out", "bend", "b"),
        C::ports("bend", "out", "pitch_rate", "bend"),
        C::ports("ratio", "out", "pitch_rate", "ratio"),
        C::ports("pitch_rate", "out", "speed", "a"),
        C::ports("pitch", "out", "speed", "b"),
        C::ports("position", "out", "position_frames", "a"),
        C::ports("frames", "out", "position_frames", "b"),
        C::ports("density", "out", "scheduler", "density"),
        C::ports("voice", "gate", "scheduler", "gate"),
        C::ports("voice", "trigger", "scheduler", "trigger"),
        C::ports("frames", "out", "spread_frames", "a"),
        C::ports("spread_amount", "out", "spread_frames", "b"),
        C::ports("position_frames", "out", "jitter", "value"),
        C::ports("spread_frames", "out", "jitter", "spread"),
        C::ports("scheduler", "out", "jitter", "trigger"),
        C::ports("frames", "out", "jitter", "max"),
        C::ports("scheduler", "out", "pool", "trigger"),
        C::ports("jitter", "out", "pool", "position"),
        C::ports("speed", "out", "pool", "rate"),
        C::ports("size", "out", "pool", "duration"),
        C::ports("pool", "out", "amp", "a"),
        C::ports("voice", "velocity", "amp", "b"),
        C::ports("voice", "gate", "released", "a"),
        C::ports("gate_threshold", "out", "released", "b"),
        C::ports("released", "out", "finished", "a"),
        C::ports("pool", "finished", "finished", "b"),
        C::new("amp", "voice_out"),
        C::ports("finished", "out", "voice_out", "finished"),
        C::new("voice_out", "out"),
    ];
    body.validate()?;
    let mut graph = DeviceGraph::new("org.suhina.granular-template", "Granular instrument", DeviceRole::Instrument);
    for (id, name, range, default) in [
        ("position", "Position", (0., 1.), 0.5),
        ("size", "Size (seconds)", (0.001, 1.), 0.12),
        ("density", "Density", (0., 1000.), 8.),
        ("pitch", "Pitch ratio", (-16., 16.), 1.),
    ] {
        graph.parameters.push(Parameter::new(id, name, range, default));
        graph.nodes.push(N::new(id, NodeKind::Parameter { parameter: id.into() }));
    }
    graph
        .nodes
        .extend([N::new("voices", NodeKind::VoiceGroup { graph: body.to_sd()?, voices }), N::audio_output("out")]);
    for (id, port) in [("position", "control1"), ("size", "control2"), ("density", "control3"), ("pitch", "control4")] {
        graph.connections.push(C::ports(id, "out", "voices", port));
    }
    graph.connections.push(C::new("voices", "out"));
    graph.validate()?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_trigger_held_high_starts_one_grain() {
        // No grain graphs: every start is dropped, so the count is the number of starts.
        let mut pool = pool(Vec::new(), 48_000.);
        let high = [1.; 128];
        let zero = [0.; 128];
        let mut out = [0.; 128];
        pool.process([&high, &zero, &zero, &zero, &zero], &mut out);
        assert_eq!(pool.output(1, 63), [1.; 2], "one start for a trigger held over 64 frames");
        // Falling and rising again starts another.
        let mut pulses = [0.; 128];
        pulses[20] = 1.;
        pulses[21] = 1.;
        pool.process([&zero, &zero, &zero, &zero, &zero], &mut out);
        pool.process([&pulses, &zero, &zero, &zero, &zero], &mut out);
        assert_eq!(pool.output(1, 63), [2.; 2]);
    }
    #[test]
    fn a_granular_player_over_the_grain_budget_fails_validation() {
        let player = |voices, grains| {
            let mut g = DeviceGraph::new("org.test.granular", "Granular", DeviceRole::Instrument);
            g.nodes = vec![
                N::new("player", NodeKind::GranularPlayer { sample: "sample".into(), voices, grains, seed: 1 }),
                N::audio_output("out"),
            ];
            g.connections = vec![C::new("player", "out")];
            g.validate().err().map(|e| e.0).unwrap_or_default()
        };
        assert!(player(8, 64).contains("at most 256 grains in all"), "{}", player(8, 64));
        assert!(!player(8, 32).contains("grains in all"), "{}", player(8, 32));
    }
    #[test]
    fn scheduler_preserves_fractional_carry() {
        let mut remaining = 0.;
        let starts = (0..10).filter(|_| tick(&mut remaining, 2.5)).collect::<Vec<_>>();
        assert_eq!(starts, [0, 2, 4, 7, 9]);
    }
}
