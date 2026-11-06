// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editable data-only sampler compositions; no alternate DSP interpreter.
use crate::{
    Connection as C, DeviceGraph, DeviceRole, DeviceSdkError, FilterMode, Node as N, NodeKind as K, Parameter,
    ParameterControl, SampleInfoField,
};
fn constant(id: &str, value: f64) -> N {
    N::new(id, K::Constant { value })
}
fn wire(g: &mut DeviceGraph, from: &str, port: &str, to: &str, input: &str) {
    g.connections.push(C::ports(from, port, to, input));
}
/// A useful polyphonic sampler authored entirely with public graph nodes.
pub fn sampler_template(sample: &str, voices: u8) -> Result<DeviceGraph, DeviceSdkError> {
    if voices == 0 || voices > 32 {
        return Err(DeviceSdkError("sampler template needs 1..32 voices".into()));
    }
    let controls = [
        ("pitch", -48., 48., 0.),
        ("cents", -100., 100., 0.),
        ("root", 0., 127., 60.),
        ("start", 0., 1., 0.),
        ("end", 0., 1., 1.),
        ("loop_start", 0., 1., 0.),
        ("loop_end", 0., 1., 1.),
        ("ping_pong", 0., 1., 0.),
        ("reverse", 0., 1., 0.),
        ("playback", 0., 2., 0.),
        ("attack", 0., 10., 0.005),
        ("decay", 0., 10., 0.22),
        ("sustain", 0., 1., 0.87),
        ("release", 0., 10., 0.48),
        ("cutoff", 20., 20000., 20000.),
        ("resonance", 0.1, 10., 0.707),
        ("velocity", 0., 1., 1.),
    ];
    let mut voice = DeviceGraph::new("org.suhina.sample-voice-template", "Sample voice", DeviceRole::Instrument);
    for (id, min, max, default) in controls {
        let mut parameter = Parameter::new(id, id, (min, max), default);
        parameter.ui.group = if matches!(id, "attack" | "decay" | "sustain" | "release") {
            "Envelope"
        } else if matches!(id, "cutoff" | "resonance") {
            "Tone"
        } else {
            "Playback"
        }
        .into();
        if id == "root" {
            parameter.control = ParameterControl::Integer;
        }
        if matches!(id, "reverse" | "ping_pong") {
            parameter.control = ParameterControl::Toggle;
        }
        if id == "playback" {
            parameter.control = ParameterControl::Choice;
            parameter.ui.options = vec!["Gate".into(), "One shot".into(), "Loop".into()];
        }
        voice.parameters.push(parameter);
        voice.nodes.push(N::new(id, K::Parameter { parameter: id.into() }));
    }
    voice.nodes.extend([
        N::new("voice", K::VoiceInput { controller: 1 }),
        N::new("asset", K::SampleAsset { sample: sample.into() }),
        N::new("frames", K::SampleInfo { field: SampleInfoField::Frames }),
        N::new("source_rate", K::SampleInfo { field: SampleInfoField::SampleRate }),
        N::new("output_rate", K::SampleRate),
        N::new("ratio", K::Divide),
        N::new("bend", K::Multiply),
        constant("two", 2.),
        N::new("transpose", K::Add),
        N::new("rate", K::PlaybackRate),
        constant("one", 1.),
        constant("minus_one", -1.),
        constant("half", 0.5),
        constant("one_half", 1.5),
        N::new("twice_reverse", K::Multiply),
        N::new("direction", K::Subtract),
        N::new("signed_rate", K::Multiply),
        N::new("region", K::SampleRegion),
        N::new("end_last", K::Add),
        N::new("initial", K::Select),
        N::new("cursor", K::PlaybackPosition),
        N::new("loop_lo", K::Max),
        N::new("loop_hi", K::Min),
        N::new("wrapped", K::LoopPosition),
        N::new("forward_initial", K::Less),
        N::new("forward", K::Select),
        N::new("reverse_wrapped", K::Less),
        N::new("backward", K::Select),
        N::new("loop_position", K::Select),
        N::new("is_loop", K::Greater),
        N::new("position", K::Select),
        N::new("relative", K::Subtract),
        N::new("read", K::SampleRead),
        N::new("bounds", K::PlaybackFinished),
        N::new("loop_valid", K::Greater),
        N::new("loop_invalid", K::Subtract),
        N::new("mode_low", K::Greater),
        N::new("mode_high", K::Less),
        N::new("one_shot", K::Multiply),
        N::new("in_bounds", K::Subtract),
        N::new("hold", K::Multiply),
        N::new("gate", K::Max),
        N::new("env", K::Adsr),
        N::new("filter", K::Filter { mode: FilterMode::Lowpass }),
        N::new("velocity_level", K::Mix),
        N::new("level", K::Multiply),
        N::new("amp", K::Multiply),
        N::new("normal_done", K::Max),
        N::new("loop_done", K::Max),
        N::new("finished", K::Select),
        N::new("voice_out", K::VoiceOutput),
        N::audio_output("out"),
    ]);
    for (id, param) in [
        ("start_frames", "start"),
        ("end_frames", "end"),
        ("loop_start_frames", "loop_start"),
        ("loop_end_frames", "loop_end"),
    ] {
        let scale = format!("{id}_scale");
        voice.nodes.push(N::new(&scale, K::Multiply));
        voice.nodes.push(N::new(id, K::Floor));
        wire(&mut voice, param, "out", &scale, "a");
        wire(&mut voice, "frames", "out", &scale, "b");
        wire(&mut voice, &scale, "out", id, "in");
    }
    for (from, port, to, input) in [
        ("asset", "out", "frames", "asset"),
        ("asset", "out", "source_rate", "asset"),
        ("source_rate", "out", "ratio", "a"),
        ("output_rate", "out", "ratio", "b"),
        ("voice", "bend", "bend", "a"),
        ("two", "out", "bend", "b"),
        ("pitch", "out", "transpose", "a"),
        ("bend", "out", "transpose", "b"),
        ("voice", "key", "rate", "key"),
        ("root", "out", "rate", "root"),
        ("cents", "out", "rate", "fine"),
        ("transpose", "out", "rate", "bend"),
        ("ratio", "out", "rate", "ratio"),
        ("reverse", "out", "twice_reverse", "a"),
        ("two", "out", "twice_reverse", "b"),
        ("one", "out", "direction", "a"),
        ("twice_reverse", "out", "direction", "b"),
        ("rate", "out", "signed_rate", "a"),
        ("direction", "out", "signed_rate", "b"),
        ("asset", "out", "region", "asset"),
        ("start_frames", "out", "region", "start"),
        ("end_frames", "out", "region", "end"),
        ("end_frames", "out", "end_last", "a"),
        ("minus_one", "out", "end_last", "b"),
        ("start_frames", "out", "initial", "a"),
        ("end_last", "out", "initial", "b"),
        ("reverse", "out", "initial", "condition"),
        ("signed_rate", "out", "cursor", "rate"),
        ("initial", "out", "cursor", "start"),
        ("voice", "trigger", "cursor", "reset"),
        ("start_frames", "out", "loop_lo", "a"),
        ("loop_start_frames", "out", "loop_lo", "b"),
        ("end_frames", "out", "loop_hi", "a"),
        ("loop_end_frames", "out", "loop_hi", "b"),
        ("cursor", "out", "wrapped", "position"),
        ("loop_lo", "out", "wrapped", "start"),
        ("loop_hi", "out", "wrapped", "end"),
        ("ping_pong", "out", "wrapped", "ping_pong"),
        ("cursor", "out", "forward_initial", "a"),
        ("loop_hi", "out", "forward_initial", "b"),
        ("wrapped", "out", "forward", "a"),
        ("cursor", "out", "forward", "b"),
        ("forward_initial", "out", "forward", "condition"),
        ("cursor", "out", "reverse_wrapped", "a"),
        ("loop_lo", "out", "reverse_wrapped", "b"),
        ("cursor", "out", "backward", "a"),
        ("wrapped", "out", "backward", "b"),
        ("reverse_wrapped", "out", "backward", "condition"),
        ("forward", "out", "loop_position", "a"),
        ("backward", "out", "loop_position", "b"),
        ("reverse", "out", "loop_position", "condition"),
        ("playback", "out", "is_loop", "a"),
        ("one_half", "out", "is_loop", "b"),
        ("cursor", "out", "position", "a"),
        ("loop_position", "out", "position", "b"),
        ("is_loop", "out", "position", "condition"),
        ("position", "out", "relative", "a"),
        ("start_frames", "out", "relative", "b"),
        ("asset", "out", "read", "asset"),
        ("relative", "out", "read", "position"),
        ("region", "out", "read", "region"),
        ("cursor", "out", "bounds", "position"),
        ("start_frames", "out", "bounds", "start"),
        ("end_frames", "out", "bounds", "end"),
        ("loop_hi", "out", "loop_valid", "a"),
        ("loop_lo", "out", "loop_valid", "b"),
        ("one", "out", "loop_invalid", "a"),
        ("loop_valid", "out", "loop_invalid", "b"),
        ("playback", "out", "mode_low", "a"),
        ("half", "out", "mode_low", "b"),
        ("playback", "out", "mode_high", "a"),
        ("one_half", "out", "mode_high", "b"),
        ("mode_low", "out", "one_shot", "a"),
        ("mode_high", "out", "one_shot", "b"),
        ("one", "out", "in_bounds", "a"),
        ("bounds", "finished", "in_bounds", "b"),
        ("one_shot", "out", "hold", "a"),
        ("in_bounds", "out", "hold", "b"),
        ("voice", "gate", "gate", "a"),
        ("hold", "out", "gate", "b"),
        ("gate", "out", "env", "gate"),
        ("attack", "out", "env", "attack"),
        ("decay", "out", "env", "decay"),
        ("sustain", "out", "env", "sustain"),
        ("release", "out", "env", "release"),
        ("read", "out", "filter", "in"),
        ("cutoff", "out", "filter", "frequency"),
        ("resonance", "out", "filter", "q"),
        ("one", "out", "velocity_level", "a"),
        ("voice", "velocity", "velocity_level", "b"),
        ("velocity", "out", "velocity_level", "amount"),
        ("env", "out", "level", "a"),
        ("velocity_level", "out", "level", "b"),
        ("filter", "out", "amp", "a"),
        ("level", "out", "amp", "b"),
        ("env", "finished", "normal_done", "a"),
        ("bounds", "finished", "normal_done", "b"),
        ("env", "finished", "loop_done", "a"),
        ("loop_invalid", "out", "loop_done", "b"),
        ("normal_done", "out", "finished", "a"),
        ("loop_done", "out", "finished", "b"),
        ("is_loop", "out", "finished", "condition"),
        ("amp", "out", "voice_out", "in"),
        ("finished", "out", "voice_out", "finished"),
        ("voice_out", "out", "out", "in"),
    ] {
        wire(&mut voice, from, port, to, input);
    }
    voice.validate()?;
    let mut graph = DeviceGraph::new("org.suhina.sampler-template", "Sample instrument", DeviceRole::Instrument);
    graph.parameters = voice.parameters.clone();
    graph
        .parameters
        .extend([Parameter::new("gain", "Gain", (-60., 12.), 0.), Parameter::new("pan", "Pan", (-1., 1.), 0.)]);
    let mut group = N::new("voices", K::VoiceGroup { graph: voice.to_sd()?, voices });
    for p in &voice.parameters {
        group = group.bind(&p.id, &p.id);
    }
    graph.nodes = vec![
        group,
        N::new("gain", K::Parameter { parameter: "gain".into() }),
        N::new("linear_gain", K::DbToGain),
        N::new("amp", K::Multiply),
        N::new("pan", K::Parameter { parameter: "pan".into() }),
        N::new("balance", K::Pan),
        N::audio_output("out"),
    ];
    graph.connections = vec![
        C::ports("gain", "out", "linear_gain", "in"),
        C::ports("voices", "out", "amp", "a"),
        C::ports("linear_gain", "out", "amp", "b"),
        C::new("amp", "balance"),
        C::ports("pan", "out", "balance", "balance"),
        C::new("balance", "out"),
    ];
    graph.validate()?;
    Ok(graph)
}
