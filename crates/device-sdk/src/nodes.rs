// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Executable node kinds and their machine-readable port contracts.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
    Random,
}
impl Waveform {
    pub(crate) fn dsp(self) -> suhina_dsp::lfo::Shape {
        match self {
            Self::Sine => suhina_dsp::lfo::Shape::Sine,
            Self::Triangle => suhina_dsp::lfo::Shape::Triangle,
            Self::SawUp => suhina_dsp::lfo::Shape::SawUp,
            Self::SawDown => suhina_dsp::lfo::Shape::SawDown,
            Self::Square => suhina_dsp::lfo::Shape::Square,
            Self::Random => suhina_dsp::lfo::Shape::Random,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Peak,
    LowShelf,
    HighShelf,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeKind {
    /// A whole-device wrapper resolved only through the host's registered factory.
    NativeDevice {
        device: String,
    },
    AudioInput,
    AudioOutput,
    MidiInput,
    MidiOutput,
    MidiTranspose {
        semitones: i8,
    },
    MidiTransposeParameter {
        parameter: String,
    },
    MidiKey,
    MidiVelocity,
    MidiGate,
    Gain {
        parameter: String,
    },
    Constant {
        value: f64,
    },
    Parameter {
        parameter: String,
    },
    Add,
    Subtract,
    Multiply,
    Divide,
    Min,
    Max,
    Power,
    Less,
    Greater,
    Abs,
    Negate,
    Sqrt,
    Exp,
    Log,
    Floor,
    Fract,
    Sin,
    Cos,
    Tanh,
    Atan,
    DbToGain,
    GainToDb,
    Clamp,
    Mix,
    Select,
    Stereo,
    Mono,
    MidSideEncode,
    MidSideDecode,
    Pan,
    Oscillator {
        shape: Waveform,
    },
    SyncedLfo {
        shape: Waveform,
    },
    Noise {
        seed: u32,
    },
    Smooth,
    Envelope {
        rms: bool,
    },
    SampleHold,
    Filter {
        mode: FilterMode,
    },
    SampleDelay,
    Delay {
        max_seconds: f64,
    },
    Tempo,
    LfoRate,
    Beat,
    Playing,
}

#[derive(Debug, Serialize)]
pub struct PortInfo {
    pub name: &'static str,
    pub default: Option<f64>,
    pub unit: &'static str,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NodeInfo {
    pub kind: &'static str,
    pub name: &'static str,
    pub inputs: &'static [PortInfo],
    pub outputs: &'static [&'static str],
    pub description: &'static str,
    pub configuration: &'static str,
    pub example: &'static str,
    pub rust_variant: &'static str,
}

const NATIVE_DEVICE: NodeInfo = NodeInfo {
    kind: "native_device",
    name: "Native device",
    rust_variant: "NativeDevice",
    inputs: &[PortInfo { name: "in", default: Some(0.), unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Whole-device native wrapper. Effects wire audio_input -> native_device -> audio_output; instruments wire native_device -> audio_output. Requires the host's registered factory via prepare_native_at; no arbitrary code or feedback. Parameters preserve the native contract. Processing/reset/events use Device's real-time-safe API.",
    configuration: "device: stable registered device id, 1..128 bytes",
    example: r#"{"kind":"native_device","device":"utility"}"#,
};
/// Catalog for humans, CLI tooling and AI agents; serialize with serde_json.
mod io;
mod math;
mod midi;
mod modulation;
mod stereo;
mod transport;
pub static NODE_CATALOG: &[NodeInfo] = &[
    NATIVE_DEVICE,
    io::AUDIO_INPUT,
    io::AUDIO_OUTPUT,
    io::GAIN,
    io::CONSTANT,
    io::PARAMETER,
    math::ADD,
    math::SUBTRACT,
    math::MULTIPLY,
    math::DIVIDE,
    math::MIN,
    math::MAX,
    math::POWER,
    math::LESS,
    math::GREATER,
    math::ABS,
    math::NEGATE,
    math::SQRT,
    math::EXP,
    math::LOG,
    math::FLOOR,
    math::FRACT,
    math::SIN,
    math::COS,
    math::TANH,
    math::ATAN,
    math::DB_TO_GAIN,
    math::GAIN_TO_DB,
    math::CLAMP,
    math::MIX,
    math::SELECT,
    stereo::STEREO,
    stereo::MONO,
    stereo::MID_SIDE_ENCODE,
    stereo::MID_SIDE_DECODE,
    stereo::PAN,
    modulation::OSCILLATOR,
    modulation::SYNCED_LFO,
    modulation::NOISE,
    modulation::SMOOTH,
    modulation::ENVELOPE,
    modulation::SAMPLE_HOLD,
    modulation::FILTER,
    modulation::SAMPLE_DELAY,
    modulation::DELAY,
    transport::TEMPO,
    transport::BEAT,
    transport::PLAYING,
    midi::MIDI_INPUT,
    midi::MIDI_OUTPUT,
    midi::MIDI_TRANSPOSE,
    midi::MIDI_TRANSPOSE_PARAMETER,
    midi::MIDI_KEY,
    midi::MIDI_VELOCITY,
    midi::MIDI_GATE,
    transport::LFO_RATE,
];

impl NodeKind {
    pub fn info(&self) -> &'static NodeInfo {
        match self {
            Self::NativeDevice { .. } => &NATIVE_DEVICE,
            Self::AudioInput => &io::AUDIO_INPUT,
            Self::AudioOutput => &io::AUDIO_OUTPUT,
            Self::Gain { .. } => &io::GAIN,
            Self::Constant { .. } => &io::CONSTANT,
            Self::Parameter { .. } => &io::PARAMETER,
            Self::Add => &math::ADD,
            Self::Subtract => &math::SUBTRACT,
            Self::Multiply => &math::MULTIPLY,
            Self::Divide => &math::DIVIDE,
            Self::Min => &math::MIN,
            Self::Max => &math::MAX,
            Self::Power => &math::POWER,
            Self::Less => &math::LESS,
            Self::Greater => &math::GREATER,
            Self::Abs => &math::ABS,
            Self::Negate => &math::NEGATE,
            Self::Sqrt => &math::SQRT,
            Self::Exp => &math::EXP,
            Self::Log => &math::LOG,
            Self::Floor => &math::FLOOR,
            Self::Fract => &math::FRACT,
            Self::Sin => &math::SIN,
            Self::Cos => &math::COS,
            Self::Tanh => &math::TANH,
            Self::Atan => &math::ATAN,
            Self::DbToGain => &math::DB_TO_GAIN,
            Self::GainToDb => &math::GAIN_TO_DB,
            Self::Clamp => &math::CLAMP,
            Self::Mix => &math::MIX,
            Self::Select => &math::SELECT,
            Self::Stereo => &stereo::STEREO,
            Self::Mono => &stereo::MONO,
            Self::MidSideEncode => &stereo::MID_SIDE_ENCODE,
            Self::MidSideDecode => &stereo::MID_SIDE_DECODE,
            Self::Pan => &stereo::PAN,
            Self::Oscillator { .. } => &modulation::OSCILLATOR,
            Self::SyncedLfo { .. } => &modulation::SYNCED_LFO,
            Self::Noise { .. } => &modulation::NOISE,
            Self::Smooth => &modulation::SMOOTH,
            Self::Envelope { .. } => &modulation::ENVELOPE,
            Self::SampleHold => &modulation::SAMPLE_HOLD,
            Self::Filter { .. } => &modulation::FILTER,
            Self::SampleDelay => &modulation::SAMPLE_DELAY,
            Self::Delay { .. } => &modulation::DELAY,
            Self::Tempo => &transport::TEMPO,
            Self::Beat => &transport::BEAT,
            Self::Playing => &transport::PLAYING,
            Self::MidiInput => &midi::MIDI_INPUT,
            Self::MidiOutput => &midi::MIDI_OUTPUT,
            Self::MidiTranspose { .. } => &midi::MIDI_TRANSPOSE,
            Self::MidiTransposeParameter { .. } => &midi::MIDI_TRANSPOSE_PARAMETER,
            Self::MidiKey => &midi::MIDI_KEY,
            Self::MidiVelocity => &midi::MIDI_VELOCITY,
            Self::MidiGate => &midi::MIDI_GATE,
            Self::LfoRate => &transport::LFO_RATE,
        }
    }
    pub(crate) fn breaks_cycle(&self, port: &str) -> bool {
        matches!(self, Self::Delay { .. } | Self::SampleDelay) && port == "in"
    }
}

/// Generated human reference; shared with the JSON catalog and checked against the spec.
pub fn node_reference_markdown() -> String {
    use std::fmt::Write;
    let mut text = String::from("| JSON kind / Rust variant | Inputs (default, unit) | Behaviour |\n|---|---|---|\n");
    for node in NODE_CATALOG {
        let ports = node
            .inputs
            .iter()
            .map(|p| {
                let default = p.default.map_or_else(|| "required".into(), |v| v.to_string());
                format!("`{}` ({}, {})", p.name, default, p.unit)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ports = if ports.is_empty() { "None" } else { &ports };
        let _written =
            writeln!(text, "| `{}` / `{}` | {} | {} |", node.kind, node.rust_variant, ports, node.description);
    }
    text
}
