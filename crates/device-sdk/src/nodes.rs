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
    /// Polyphonic sample source; resources are bound off the audio thread.
    Sampler,
    SamplerAsset {
        sample: String,
    },
    /// Trusted host-provided graph effect with five fixed signal ports.
    NativeEffect {
        effect: String,
    },
    SampleAsset {
        sample: String,
    },
    SampleInfo {
        field: SampleInfoField,
    },
    SampleRegion,
    SampleRead,
    SampleTempo {
        beats: bool,
    },
    SliceMap {
        sample: String,
        boundaries: Vec<u64>,
    },
    SliceSelect,
    KeyRange {
        min: u8,
        max: u8,
    },
    VelocityRange {
        min: f32,
        max: f32,
    },
    Sustain,
    VoiceGroup {
        graph: String,
        voices: u8,
    },
    Component {
        name: String,
        version: u32,
        graph: String,
    },
    ComponentInput {
        index: u8,
    },
    VoiceInput {
        controller: u8,
    },
    VoiceOutput,
    MidiController {
        number: u8,
    },
    PitchBend,
    Pressure,
    Adsr,
    Ad,
    PlaybackPosition,
    PlaybackBounds,
    LoopPosition,
    PlaybackRate,
    RegionFade,
    RangeMap,
    Curve,
    BeatToPosition,
    TempoRatio,
    DcBlock,
    Lfo {
        shape: Waveform,
    },
    ScanPosition,
    PositionReset,
    PlaybackFinished,
    KeyTracking,
    VelocityCurve,
    Jitter {
        seed: u32,
    },
    SampleRate,
    Glide,
    GrainScheduler,
    GrainRead,
    GrainPool {
        graph: String,
        grains: u16,
    },
    GranularPlayer {
        sample: String,
        voices: u8,
        grains: u16,
        seed: u32,
    },
    TimeStretchPlayer {
        sample: String,
    },
    SlicePlayback {
        sample: String,
        one_shot: bool,
    },
    LoopCrossfade,
    KeyToSlice,
    SliceSequence,
    RandomSlice {
        seed: u32,
    },
    RandomHold {
        seed: u32,
    },
    RandomSpread {
        seed: u32,
    },
    GrainWindow,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleInfoField {
    Frames,
    Duration,
    SampleRate,
    Channels,
}

#[derive(Debug, Serialize)]
pub struct PortInfo {
    pub name: &'static str,
    pub default: Option<f64>,
    pub unit: &'static str,
}
#[derive(Debug, Clone, Copy)]
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

impl NodeInfo {
    /// Public port types used by validation and authoring tools. Numeric signals carry the input units.
    pub fn output_type(&self, port: &str) -> Option<&'static str> {
        if !self.outputs.contains(&port) {
            return None;
        }
        Some(if port != "out" {
            "signal"
        } else {
            match self.kind {
                "midi_input"
                | "midi_transpose"
                | "midi_transpose_parameter"
                | "key_range"
                | "velocity_range"
                | "sustain" => "midi",
                "sample_asset" => "sample",
                "slice_map" => "slice_map",
                "sample_region" | "slice_select" => "region",
                _ => "signal",
            }
        })
    }
}
impl Serialize for NodeInfo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("NodeInfo", 9)?;
        state.serialize_field("kind", self.kind)?;
        state.serialize_field("name", self.name)?;
        state.serialize_field("inputs", self.inputs)?;
        state.serialize_field("outputs", self.outputs)?;
        let types = self
            .outputs
            .iter()
            .map(|&p| (p, self.output_type(p).unwrap_or("signal")))
            .collect::<std::collections::BTreeMap<_, _>>();
        state.serialize_field("output_types", &types)?;
        state.serialize_field("description", self.description)?;
        state.serialize_field("configuration", self.configuration)?;
        state.serialize_field("example", self.example)?;
        state.serialize_field("rust_variant", self.rust_variant)?;
        state.end()
    }
}

pub(crate) mod audio;
pub(crate) mod component;
pub(crate) mod envelopes;
pub(crate) mod events;
pub(crate) mod factory;
pub(crate) mod granular;
mod io;
mod math;
mod midi;
mod modulation;
mod native;
mod playback;
mod processor;
pub(crate) mod random;
/// Catalog for humans, CLI tooling and AI agents; serialize with serde_json.
pub(crate) mod resources;
pub(crate) mod sample;
pub(crate) mod slicing;
mod stereo;
pub(crate) mod stretch;
pub(crate) mod templates;
mod transport;
pub(crate) mod voice;
pub static NODE_CATALOG: &[NodeInfo] = &[
    native::NATIVE_EFFECT,
    sample::SAMPLER,
    sample::SAMPLER_ASSET,
    events::KEY_RANGE,
    events::VELOCITY_RANGE,
    events::SUSTAIN,
    voice::VOICE_GROUP,
    component::COMPONENT,
    component::COMPONENT_INPUT,
    voice::VOICE_INPUT,
    voice::VOICE_OUTPUT,
    voice::MIDI_CONTROLLER,
    voice::PITCH_BEND,
    voice::PRESSURE,
    envelopes::ADSR,
    envelopes::AD,
    resources::SAMPLE_ASSET,
    resources::SAMPLE_INFO,
    resources::SAMPLE_REGION,
    resources::SAMPLE_READ,
    resources::SAMPLE_TEMPO,
    resources::SLICE_MAP,
    resources::SLICE_SELECT,
    playback::PLAYBACK_POSITION,
    playback::PLAYBACK_BOUNDS,
    playback::LOOP_POSITION,
    playback::PLAYBACK_RATE,
    playback::REGION_FADE,
    modulation::RANGE_MAP,
    modulation::CURVE,
    transport::BEAT_TO_POSITION,
    transport::TEMPO_RATIO,
    slicing::KEY_TO_SLICE,
    slicing::SLICE_SEQUENCE,
    slicing::RANDOM_SLICE,
    random::RANDOM_HOLD,
    random::RANDOM_SPREAD,
    audio::DC_BLOCK,
    modulation::LFO,
    playback::SCAN_POSITION,
    playback::POSITION_RESET,
    playback::PLAYBACK_FINISHED,
    modulation::KEY_TRACKING,
    modulation::VELOCITY_CURVE,
    random::JITTER,
    transport::SAMPLE_RATE,
    modulation::GLIDE,
    granular::GRAIN_SCHEDULER,
    granular::GRAIN_POOL,
    granular::GRANULAR_PLAYER,
    resources::GRAIN_READ,
    resources::SLICE_PLAYBACK,
    resources::LOOP_CROSSFADE,
    stretch::TIME_STRETCH_PLAYER,
    granular::GRAIN_WINDOW,
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
            Self::Sampler => &sample::SAMPLER,
            Self::SamplerAsset { .. } => &sample::SAMPLER_ASSET,
            Self::KeyRange { .. } => &events::KEY_RANGE,
            Self::VelocityRange { .. } => &events::VELOCITY_RANGE,
            Self::Sustain => &events::SUSTAIN,
            Self::VoiceGroup { .. } => &voice::VOICE_GROUP,
            Self::Component { .. } => &component::COMPONENT,
            Self::ComponentInput { .. } => &component::COMPONENT_INPUT,
            Self::VoiceInput { .. } => &voice::VOICE_INPUT,
            Self::VoiceOutput => &voice::VOICE_OUTPUT,
            Self::MidiController { .. } => &voice::MIDI_CONTROLLER,
            Self::PitchBend => &voice::PITCH_BEND,
            Self::Pressure => &voice::PRESSURE,
            Self::Adsr => &envelopes::ADSR,
            Self::Ad => &envelopes::AD,
            Self::SampleAsset { .. } => &resources::SAMPLE_ASSET,
            Self::SampleInfo { .. } => &resources::SAMPLE_INFO,
            Self::SampleRegion => &resources::SAMPLE_REGION,
            Self::SampleRead => &resources::SAMPLE_READ,
            Self::SampleTempo { .. } => &resources::SAMPLE_TEMPO,
            Self::SliceMap { .. } => &resources::SLICE_MAP,
            Self::SliceSelect => &resources::SLICE_SELECT,
            Self::PlaybackPosition => &playback::PLAYBACK_POSITION,
            Self::PlaybackBounds => &playback::PLAYBACK_BOUNDS,
            Self::LoopPosition => &playback::LOOP_POSITION,
            Self::PlaybackRate => &playback::PLAYBACK_RATE,
            Self::RegionFade => &playback::REGION_FADE,
            Self::RangeMap => &modulation::RANGE_MAP,
            Self::Curve => &modulation::CURVE,
            Self::BeatToPosition => &transport::BEAT_TO_POSITION,
            Self::TempoRatio => &transport::TEMPO_RATIO,
            Self::DcBlock => &audio::DC_BLOCK,
            Self::Lfo { .. } => &modulation::LFO,
            Self::ScanPosition => &playback::SCAN_POSITION,
            Self::PositionReset => &playback::POSITION_RESET,
            Self::PlaybackFinished => &playback::PLAYBACK_FINISHED,
            Self::KeyTracking => &modulation::KEY_TRACKING,
            Self::VelocityCurve => &modulation::VELOCITY_CURVE,
            Self::Jitter { .. } => &random::JITTER,
            Self::SampleRate => &transport::SAMPLE_RATE,
            Self::Glide => &modulation::GLIDE,
            Self::GrainScheduler => &granular::GRAIN_SCHEDULER,
            Self::GrainRead => &resources::GRAIN_READ,
            Self::GrainPool { .. } => &granular::GRAIN_POOL,
            Self::GranularPlayer { .. } => &granular::GRANULAR_PLAYER,
            Self::TimeStretchPlayer { .. } => &stretch::TIME_STRETCH_PLAYER,
            Self::SlicePlayback { .. } => &resources::SLICE_PLAYBACK,
            Self::LoopCrossfade => &resources::LOOP_CROSSFADE,
            Self::KeyToSlice => &slicing::KEY_TO_SLICE,
            Self::SliceSequence => &slicing::SLICE_SEQUENCE,
            Self::RandomSlice { .. } => &slicing::RANDOM_SLICE,
            Self::RandomHold { .. } => &random::RANDOM_HOLD,
            Self::RandomSpread { .. } => &random::RANDOM_SPREAD,
            Self::GrainWindow => &granular::GRAIN_WINDOW,

            Self::NativeEffect { .. } => &native::NATIVE_EFFECT,
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

pub(crate) fn prepare(kind: &NodeKind, rate: f64) -> Option<Box<dyn crate::native_node::NativeNode>> {
    audio::prepare(kind, rate)
        .or_else(|| modulation::prepare_lfo(kind, rate))
        .or_else(|| transport::prepare_rate(kind, rate))
        .or_else(|| modulation::prepare_glide(kind, rate))
        .or_else(|| granular::prepare_scheduler(kind, rate))
        .or_else(|| playback::prepare(kind))
        .or_else(|| modulation::prepare(kind))
        .or_else(|| transport::prepare(kind))
        .or_else(|| granular::prepare(kind))
}

impl NodeKind {
    pub fn requires_samples(&self) -> bool {
        matches!(
            self,
            Self::SamplerAsset { .. }
                | Self::GrainRead
                | Self::SlicePlayback { .. }
                | Self::LoopCrossfade
                | Self::TimeStretchPlayer { .. }
                | Self::GranularPlayer { .. }
                | Self::SampleAsset { .. }
                | Self::SampleInfo { .. }
                | Self::SampleRegion
                | Self::SampleRead
                | Self::SampleTempo { .. }
                | Self::SliceMap { .. }
                | Self::SliceSelect
        )
    }
    pub(crate) fn resource_output(&self) -> Option<&'static str> {
        self.info().output_type("out").filter(|&kind| kind != "signal")
    }
}

impl NodeKind {
    pub(crate) fn auxiliary_count(&self) -> usize {
        self.info().outputs.iter().filter(|&&name| !matches!(name, "out" | "left" | "right")).count()
    }
    pub(crate) fn output_index(&self, name: &str) -> usize {
        if matches!(name, "out" | "left" | "right") {
            0
        } else {
            self.info()
                .outputs
                .iter()
                .filter(|&&n| !matches!(n, "out" | "left" | "right"))
                .position(|&n| n == name)
                .map_or(0, |i| i + 1)
        }
    }
}

impl NodeKind {
    pub fn is_instrument_event_node(&self) -> bool {
        matches!(
            self,
            Self::GranularPlayer { .. }
                | Self::TimeStretchPlayer { .. }
                | Self::SlicePlayback { .. }
                | Self::GrainRead
                | Self::VoiceGroup { .. }
                | Self::KeyRange { .. }
                | Self::VelocityRange { .. }
                | Self::Sustain
                | Self::VoiceInput { .. }
                | Self::MidiController { .. }
                | Self::PitchBend
                | Self::Pressure
                | Self::VoiceOutput
        )
    }
}

impl NodeKind {
    pub(crate) fn output_resource(&self, port: &str) -> Option<&'static str> {
        if port == "out" { self.resource_output() } else { None }
    }
}
