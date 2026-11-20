// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use super::*;
use crate::nodes::Waveform;
use crate::{Connection as C, Node as N, NodeKind as K};
use suhina_device_api::{ShapeKind, conformance};

fn expression_graph(output: &str) -> DeviceGraph {
    let mut voice = DeviceGraph::new("org.test.mpe-voice", "Voice", DeviceRole::Instrument);
    voice.nodes = vec![
        N::new("voice", K::VoiceInput { controller: 1 }),
        N::new("done", K::Constant { value: 0. }),
        N::new("result", K::VoiceOutput),
        N::audio_output("out"),
    ];
    voice.connections = vec![
        C::ports("voice", output, "result", "in"),
        C::ports("done", "out", "result", "finished"),
        C::new("result", "out"),
    ];
    let mut graph = DeviceGraph::new("org.test.mpe-group", "Group", DeviceRole::Instrument);
    graph.nodes =
        vec![N::new("group", K::VoiceGroup { graph: voice.to_sd().unwrap(), voices: 2 }), N::audio_output("out")];
    graph.connections = vec![C::new("group", "out")];
    graph
}

#[test]
fn mpe_voice_groups_receive_independent_pitch_pressure_and_timbre_signals() {
    use suhina_device_api::{NoteExpressionKind as Kind, NoteExpressionState, NoteInstanceEvent as E, NoteInstanceId};
    for (output, kind, a, b) in
        [("pitch", Kind::Pitch, 12., -4.), ("pressure", Kind::Pressure, 0.2, 0.7), ("timbre", Kind::Timbre, 0.3, 0.8)]
    {
        let graph = expression_graph(output);
        let mut device = audio(&graph);
        let ids = [NoteInstanceId::from_parts(1, 1).unwrap(), NoteInstanceId::from_parts(1, 2).unwrap()];
        for (id, value) in ids.into_iter().zip([a, b]) {
            let mut expression = NoteExpressionState::NEUTRAL;
            expression.set(kind, value);
            device.note_instances().unwrap().event(id, E::On { key: 60, velocity: 1., expression }).unwrap();
        }
        let mut buffer = [0.; 34];
        device.process(&ProcessContext::default(), &mut buffer);
        assert!(buffer.iter().all(|sample| (*sample - (a + b) as f32).abs() < 1e-6), "{output}: {buffer:?}");
        device.note_instances().unwrap().event(ids[0], E::Choke { key: 60 }).unwrap();
        device.process(&ProcessContext::default(), &mut buffer);
        assert!(buffer.iter().all(|sample| (*sample - b as f32).abs() < 1e-6));
        let mut finished = [None; 2];
        assert_eq!(device.note_instances().unwrap().drain_finished(&mut finished), 1);
        assert_eq!(finished[0], Some(ids[0]));
        device.reset();
        assert_eq!(device.note_instances().unwrap().drain_finished(&mut finished), 1);
        assert_eq!(finished[0], Some(ids[1]));
    }
}

#[test]
fn mpe_voice_on_preserves_shared_controller_latches_and_its_own_expression() {
    use suhina_device_api::{NoteExpressionState, NoteInstanceEvent as E, NoteInstanceId};
    for (output, control, expected) in [
        ("controller", NoteEvent::Controller { number: 1, value: 0.75 }, 1.5),
        ("bend", NoteEvent::PitchBend(0.75), 1.5),
        ("pitch", NoteEvent::PitchBend(0.75), 24.),
        ("pressure", NoteEvent::Pressure(0.4), 1.1),
    ] {
        let graph = expression_graph(output);
        let mut device = audio(&graph);
        device.event(control);
        for serial in 1..=2 {
            let id = NoteInstanceId::from_parts(7, serial).unwrap();
            let expression = NoteExpressionState::new(12., if serial == 1 { 0.2 } else { 0.7 }, 0.5).unwrap();
            device.note_instances().unwrap().event(id, E::On { key: 60, velocity: 1., expression }).unwrap();
        }
        let mut buffer = [0.; 34];
        device.process(&ProcessContext::default(), &mut buffer);
        assert!(buffer.iter().all(|sample| (*sample - expected).abs() < 1e-6), "{output}: {buffer:?}");
        if output == "pressure" {
            device.event(NoteEvent::Pressure(0.1));
            device.process(&ProcessContext::default(), &mut buffer);
            assert!(
                buffer.iter().all(|sample| (*sample - 0.9).abs() < 1e-6),
                "per-note pressure survives channel changes"
            );
        }
    }
}

#[test]
fn mpe_voice_group_with_every_voice_held_takes_the_oldest_and_retires_it_after_the_fade() {
    use suhina_device_api::{NoteExpressionState, NoteInstanceEvent as E, NoteInstanceId};
    let graph = expression_graph("pitch");
    let mut device = audio(&graph);
    let ids: Vec<_> = (1..=3).map(|serial| NoteInstanceId::from_parts(5, serial).unwrap()).collect();
    for (id, pitch) in ids.iter().zip([12., -4., 5.]) {
        let expression = NoteExpressionState::new(pitch, 0., 0.5).unwrap();
        device.note_instances().unwrap().event(*id, E::On { key: 60, velocity: 1., expression }).unwrap();
    }
    // The third note took the oldest voice: after its 64-frame fade only the two newest sound.
    let mut buffer = [0.; 128];
    device.process(&ProcessContext::default(), &mut buffer);
    device.process(&ProcessContext::default(), &mut buffer);
    println!("after fade: {:?}", &buffer[..4]);
    assert!(buffer.iter().all(|sample| (*sample - 1.).abs() < 1e-6), "{buffer:?}");
    let mut finished = [None; 4];
    assert_eq!(device.note_instances().unwrap().drain_finished(&mut finished), 1);
    assert_eq!(finished[0], Some(ids[0]));
    device.reset();
    let rest = device.note_instances().unwrap().drain_finished(&mut finished);
    let mut rest: Vec<_> = finished[..rest].to_vec();
    rest.sort_by_key(|id| id.map(|id| id.serial()));
    assert_eq!(rest, [Some(ids[1]), Some(ids[2])]);
}

#[test]
fn mpe_capability_rejects_private_sample_lifetimes_including_components() {
    use crate::sample_resource::{PreparedSamples, SampleAsset};
    use std::sync::Arc;

    for nested in [false, true] {
        let mut body = DeviceGraph::new("org.test.sample-body", "Sample body", DeviceRole::Instrument);
        body.nodes = vec![N::new("sample", K::SamplerAsset { sample: "tone".into() }), N::audio_output("out")];
        body.connections = vec![C::new("sample", "out")];
        let mut voice = if nested {
            let mut wrapper = DeviceGraph::new("org.test.sample-component", "Component", DeviceRole::Instrument);
            wrapper.nodes = vec![
                N::new("sample", K::Component { name: "Source".into(), version: 1, graph: body.to_sd().unwrap() }),
                N::new("midi", K::MidiInput),
                N::audio_output("out"),
            ];
            wrapper
        } else {
            body
        };
        voice.nodes.extend([N::new("done", K::Constant { value: 0. }), N::new("result", K::VoiceOutput)]);
        voice.connections =
            vec![C::new("sample", "result"), C::ports("done", "out", "result", "finished"), C::new("result", "out")];
        if nested {
            voice.connections.push(C::ports("midi", "out", "sample", "events"));
        }
        let mut graph = DeviceGraph::new("org.test.sample-group", "Group", DeviceRole::Instrument);
        graph.nodes =
            vec![N::new("group", K::VoiceGroup { graph: voice.to_sd().unwrap(), voices: 1 }), N::audio_output("out")];
        graph.connections = vec![C::new("group", "out")];
        let mut samples = PreparedSamples::default();
        samples.insert("tone", Arc::new(SampleAsset::new(Arc::new(vec![0.25; 4096]), 48000, 1).unwrap())).unwrap();
        let compiled = graph.clone();
        let mut device =
            GraphDevice::audio(&graph, 48000., Box::new(move |rate| compiled.compile_with_samples_at(rate, &samples)))
                .unwrap();
        assert!(device.note_instances().is_none(), "nested={nested}");
        device.event(NoteEvent::On { key: 60, velocity: 1. });
        let mut buffer = [0.; 128];
        device.process(&ProcessContext::default(), &mut buffer);
        assert!(buffer.iter().any(|sample| *sample > 0.), "ordinary compatibility remains playable");
    }
}

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> DeviceGraph {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap();
    DeviceGraph::from_sd(&text).unwrap()
}

fn audio(graph: &DeviceGraph) -> GraphDevice {
    made_at(graph, 48_000.0).unwrap()
}

fn made_at(graph: &DeviceGraph, rate: f64) -> Result<GraphDevice, DeviceSdkError> {
    let compiled = graph.clone();
    let compile: Compile = if graph.role == DeviceRole::Instrument {
        Box::new(move |rate| compiled.compile_instrument_at(rate))
    } else {
        Box::new(move |rate| compiled.compile_at(rate))
    };
    GraphDevice::audio(graph, rate, compile)
}

/// The graph as a device made at `rate`, as the conformance checks make one.
fn at(graph: &DeviceGraph, rate: f64) -> Result<Box<dyn Device>, suhina_device_api::DeviceError> {
    made_at(graph, rate)
        .map(|device| Box::new(device) as Box<dyn Device>)
        .map_err(|_| suhina_device_api::DeviceError::InvalidSampleRate(rate))
}

/// An instrument of four voices, each a 220 Hz sine under an envelope, louder with velocity.
fn sine_instrument() -> DeviceGraph {
    let mut voice = DeviceGraph::new("org.test.voice", "Voice", DeviceRole::Instrument);
    voice.nodes = vec![
        N::new("voice", K::VoiceInput { controller: 1 }),
        N::new("env", K::Adsr),
        N::new("attack", K::Constant { value: 0.005 }),
        N::new("decay", K::Constant { value: 0.0 }),
        N::new("sustain", K::Constant { value: 1.0 }),
        N::new("release", K::Constant { value: 0.01 }),
        N::new("freq", K::Constant { value: 220.0 }),
        N::new("osc", K::Oscillator { shape: Waveform::Sine }),
        N::new("level", K::Multiply),
        N::new("amp", K::Multiply),
        N::new("voice_out", K::VoiceOutput),
        N::audio_output("out"),
    ];
    voice.connections = vec![
        C::ports("voice", "gate", "env", "gate"),
        C::ports("attack", "out", "env", "attack"),
        C::ports("decay", "out", "env", "decay"),
        C::ports("sustain", "out", "env", "sustain"),
        C::ports("release", "out", "env", "release"),
        C::ports("freq", "out", "osc", "frequency"),
        C::ports("voice", "velocity", "level", "a"),
        C::ports("env", "out", "level", "b"),
        C::ports("osc", "out", "amp", "a"),
        C::ports("level", "out", "amp", "b"),
        C::ports("amp", "out", "voice_out", "in"),
        C::ports("env", "finished", "voice_out", "finished"),
        C::new("voice_out", "out"),
    ];
    let mut g = DeviceGraph::new("org.test.sine", "Sine", DeviceRole::Instrument);
    g.nodes = vec![N::new("group", K::VoiceGroup { graph: voice.to_sd().unwrap(), voices: 4 }), N::audio_output("out")];
    g.connections = vec![C::new("group", "out")];
    g
}

/// A MIDI effect that moves notes by its "Semitones" parameter.
fn transpose() -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.transpose", "Transpose", DeviceRole::MidiEffect);
    let mut semitones = Parameter::new("semitones", "Semitones", (-24.0, 24.0), 12.0);
    semitones.control = ParameterControl::Integer;
    semitones.unit = "st".into();
    g.parameters.push(semitones);
    g.nodes = vec![
        N::new("in", K::MidiInput),
        N::new("node", K::MidiTransposeParameter { parameter: "semitones".into() }),
        N::new("out", K::MidiOutput),
    ];
    g.connections = vec![C::new("in", "node"), C::new("node", "out")];
    g
}

#[test]
fn a_graph_describes_itself_from_its_parameters() {
    let mut graph = fixture("authoring/filter.sd");
    let mut mode = Parameter::new("mode", "Mode", (0.0, 2.0), 1.0);
    mode.control = ParameterControl::Choice;
    mode.ui.options = vec!["Low".into(), "Band".into(), "High".into()];
    let mut steps = Parameter::new("steps", "Steps", (0.0, 3.0), 0.0);
    steps.control = ParameterControl::Integer;
    let mut on = Parameter::new("on", "On", (0.0, 1.0), 1.0);
    on.control = ParameterControl::Button;
    graph.parameters.extend([mode, steps, on]);

    let info = describe(&graph);
    assert_eq!((&*info.id, &*info.name, info.role), ("org.example.filter", "filter", Role::Effect));
    let kinds: Vec<_> = info.params.iter().map(|p| (&*p.id, p.shape().kind, p.unit)).collect();
    assert_eq!(
        kinds,
        [
            ("cutoff", ShapeKind::Continuous(Scale::Logarithmic), Unit::Hertz),
            ("mode", ShapeKind::Choice, Unit::None),
            ("steps", ShapeKind::Integer, Unit::None),
            ("on", ShapeKind::Toggle, Unit::None),
        ]
    );
    let ParamKind::Choice(labels) = &info.params[1].kind else { panic!("mode is a choice") };
    assert_eq!(labels.iter().collect::<Vec<_>>(), ["Low", "Band", "High"]);
    assert_eq!(audio(&graph).info(), &info);
    assert_eq!(audio(&graph).latency(), 0);
}

#[test]
fn an_effect_sounds_as_its_prepared_graph() {
    let graph = fixture("authoring/filter.sd");
    let mut device = audio(&graph);
    let mut prepared = graph.compile_at(48_000.0).unwrap();
    device.set_param(0, 300.0);
    prepared.set_parameter(0, 300.0);
    assert_eq!(device.param(0), Some(300.0));
    let signal: Vec<f32> = (0..2_048).map(|i| ((i * 7919) % 200) as f32 / 100.0 - 1.0).collect();
    let (mut a, mut b) = (signal.clone(), signal);
    let context = ProcessContext::default();
    device.process(&context, &mut a);
    prepared.process_context(&context, &mut b);
    assert_eq!(a, b);
}

#[test]
fn preparing_again_keeps_the_parameters() {
    let graph = fixture("authoring/filter.sd");
    let mut device = audio(&graph);
    device.set_param(0, 300.0);
    device.prepare(96_000.0).unwrap();
    assert_eq!(device.param(0), Some(300.0));
    assert!(device.prepare(f64::NAN).is_err());
}

#[test]
fn a_midi_effect_moves_notes_and_keeps_them_in_the_span() {
    let mut device = GraphDevice::midi(&transpose()).unwrap();
    assert_eq!(device.info().role, Role::MidiEffect);
    let input = [(3, NoteEvent::On { key: 60, velocity: 1.0 }), (90, NoteEvent::Off { key: 60, velocity: 0.0 })];
    let mut output = [(0, NoteEvent::AllNotesOff); 4];
    let written = device.process_events(&ProcessContext::default(), 64, &input, &mut output);
    assert_eq!(
        output[..written],
        [(3, NoteEvent::On { key: 72, velocity: 1.0 }), (63, NoteEvent::Off { key: 72, velocity: 0.0 })]
    );
    device.set_param(0, -3.0);
    assert_eq!(device.param(0), Some(-3.0));
}

#[test]
fn graphs_pass_the_device_conformance_checks() {
    let effects = ["authoring/filter.sd", "authoring/delay.sd", "authoring/lfo-pan.sd", "gain-v1.sd"];
    let mut failures = Vec::new();
    for name in effects {
        let graph = fixture(name);
        let found = conformance::check(&|rate| at(&graph, rate));
        failures.extend(found.into_iter().map(|f| format!("{name}: {f}")));
    }
    let instrument = sine_instrument();
    let found = conformance::check(&|rate| at(&instrument, rate));
    failures.extend(found.into_iter().map(|f| format!("sine instrument: {f}")));
    let midi = transpose();
    let found = conformance::check(&|rate| {
        suhina_device_api::check_sample_rate(rate)?;
        Ok(Box::new(GraphDevice::midi(&midi).unwrap()))
    });
    failures.extend(found.into_iter().map(|f| format!("transpose: {f}")));
    assert!(failures.is_empty(), "{failures:#?}");
}
