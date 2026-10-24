// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub const TEMPO: NodeInfo = NodeInfo {
    kind: "tempo",
    name: "Tempo",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts current transport tempo in beats/minute, limited to 1..1000.",
    configuration: "none",
    example: "{\"kind\":\"tempo\"}",
    rust_variant: "Tempo",
};

pub const BEAT: NodeInfo = NodeInfo {
    kind: "beat",
    name: "Beat",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts transport beat, advanced each frame while playing.",
    configuration: "none",
    example: "{\"kind\":\"beat\"}",
    rust_variant: "Beat",
};

pub const PLAYING: NodeInfo = NodeInfo {
    kind: "playing",
    name: "Playing",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts transport running flag as 0 or 1.",
    configuration: "none",
    example: "{\"kind\":\"playing\"}",
    rust_variant: "Playing",
};

pub const LFO_RATE: NodeInfo = NodeInfo {
    kind: "lfo_rate",
    name: "LFO Rate",
    inputs: &[
        PortInfo { name: "rate", default: Some(0.5), unit: "Hz" },
        PortInfo { name: "time", default: Some(2000.0), unit: "ms" },
        PortInfo { name: "division", default: Some(4.0), unit: "divisor or sixteenth count" },
        PortInfo { name: "mode", default: Some(0.0), unit: "0 Rate, 1 Time, 2 Synced, 3 Triplet, 4 Dotted, 5 16th" },
        PortInfo { name: "tempo", default: Some(120.0), unit: "BPM" },
    ],
    outputs: &["out", "left", "right"],
    description: "Stateless Hz conversion. Clamps rate to 0.01..20, time to 1..20000 ms, rounded division to 1..64, rounded mode to 0..5 and tempo to 1..1000 BPM. Modes output rate, 1000/time, tempo*division/240, tempo*division/160, tempo*division/360 or tempo/(15*division). Broadcasts mono; no reset state.",
    configuration: "none",
    example: "{\"kind\":\"lfo_rate\"}",
    rust_variant: "LfoRate",
};

pub(crate) const BEAT_TO_POSITION: NodeInfo = NodeInfo {
    kind: "beat_to_position",
    name: "BeatToPosition",
    rust_variant: "BeatToPosition",
    inputs: &[
        PortInfo { name: "beat", default: Some(0.0), unit: "signal" },
        PortInfo { name: "bpm", default: Some(120.0), unit: "signal" },
        PortInfo { name: "sample_rate", default: Some(48000.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Source beat position to sample frames.",
    configuration: "none",
    example: r#"{"kind":"beat_to_position"}"#,
};
fn beat_to_position(x: [f64; 5]) -> f64 {
    if x[1] > 0. && x[2] > 0. { x[0] * 60. / x[1] * x[2] } else { 0. }
}

pub(crate) const TEMPO_RATIO: NodeInfo = NodeInfo {
    kind: "tempo_ratio",
    name: "TempoRatio",
    rust_variant: "TempoRatio",
    inputs: &[
        PortInfo { name: "source_bpm", default: Some(120.0), unit: "signal" },
        PortInfo { name: "target_bpm", default: Some(120.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Target tempo divided by positive source tempo.",
    configuration: "none",
    example: r#"{"kind":"tempo_ratio"}"#,
};
fn tempo_ratio(x: [f64; 5]) -> f64 {
    if x[0] > 0. { x[1] / x[0] } else { 0. }
}

pub(crate) fn prepare(kind: &super::NodeKind) -> Option<Box<dyn crate::native_node::NativeNode>> {
    match kind {
        super::NodeKind::BeatToPosition => Some(super::processor::boxed(beat_to_position)),
        super::NodeKind::TempoRatio => Some(super::processor::boxed(tempo_ratio)),
        _ => None,
    }
}

pub(crate) const SAMPLE_RATE: NodeInfo = NodeInfo {
    kind: "sample_rate",
    name: "SampleRate",
    rust_variant: "SampleRate",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Current graph processing sample rate in Hz.",
    configuration: "none",
    example: r#"{"kind":"sample_rate"}"#,
};
struct Rate(f32);
pub(crate) fn prepare_rate(kind: &super::NodeKind, rate: f64) -> Option<Box<dyn crate::native_node::NativeNode>> {
    matches!(kind, super::NodeKind::SampleRate)
        .then(|| Box::new(Rate(rate as f32)) as Box<dyn crate::native_node::NativeNode>)
}
impl crate::native_node::NativeNode for Rate {
    fn prepare_block(&mut self, _: &crate::ProcessContext) {}
    fn process(&mut self, _: [&[f32]; 5], output: &mut [f32]) {
        output.fill(self.0);
    }
    fn event(&mut self, _: suhina_device_api::NoteEvent) {}
    fn reset(&mut self) {}
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
