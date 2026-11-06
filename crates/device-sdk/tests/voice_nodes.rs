// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use suhina_device_api::NoteEvent as E;
use suhina_device_sdk::{Connection as C, DeviceGraph, DeviceRole, Node as N, NodeKind as K, ProcessContext};
fn constant(id: &str, value: f64) -> N {
    N::new(id, K::Constant { value })
}
fn voice() -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.voice", "Voice", DeviceRole::Instrument);
    g.nodes = vec![
        N::new("voice", K::VoiceInput { controller: 1 }),
        N::new("env", K::Adsr),
        constant("attack", 0.),
        constant("decay", 0.),
        constant("sustain", 1.),
        constant("release", 0.001),
        N::new("amp", K::Multiply),
        N::new("voice_out", K::VoiceOutput),
        N::audio_output("out"),
    ];
    g.connections = vec![
        C::ports("voice", "gate", "env", "gate"),
        C::ports("attack", "out", "env", "attack"),
        C::ports("decay", "out", "env", "decay"),
        C::ports("sustain", "out", "env", "sustain"),
        C::ports("release", "out", "env", "release"),
        C::ports("voice", "velocity", "amp", "a"),
        C::ports("env", "out", "amp", "b"),
        C::ports("amp", "out", "voice_out", "in"),
        C::ports("env", "finished", "voice_out", "finished"),
        C::new("voice_out", "out"),
    ];
    g
}
fn instrument(voices: u8, filters: bool) -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.group", "Group", DeviceRole::Instrument);
    g.nodes = vec![N::new("group", K::VoiceGroup { graph: voice().to_sd().unwrap(), voices }), N::audio_output("out")];
    g.connections = vec![C::new("group", "out")];
    if filters {
        g.nodes.extend([
            N::new("midi", K::MidiInput),
            N::new("keys", K::KeyRange { min: 60, max: 72 }),
            N::new("velocity", K::VelocityRange { min: 0.2, max: 1. }),
            N::new("transpose", K::MidiTranspose { semitones: 12 }),
            N::new("pedal", K::Sustain),
        ]);
        g.connections.extend([
            C::new("midi", "keys"),
            C::new("keys", "velocity"),
            C::new("velocity", "transpose"),
            C::new("transpose", "pedal"),
            C::ports("pedal", "out", "group", "events"),
        ]);
    }
    g
}
fn render(graph: &DeviceGraph, events: &[(usize, E)], frames: usize, block: usize) -> Vec<f32> {
    let mut prepared = graph.compile_instrument_at(48000.).unwrap();
    let mut output = vec![0.; frames * 2];
    for (index, chunk) in output.chunks_mut(block * 2).enumerate() {
        let offset = index * block;
        let end = offset + chunk.len() / 2;
        let local: Vec<_> =
            events.iter().filter(|(at, _)| *at >= offset && *at < end).map(|(at, e)| (*at - offset, *e)).collect();
        prepared.process_instrument_context(&ProcessContext::default(), &local, chunk);
    }
    assert_eq!(prepared.event_overflows(), 0);
    output
}
#[test]
fn private_voices_sum_release_and_pedal_at_exact_event_frames() {
    let graph = instrument(4, true);
    let events = [
        (0, E::On { key: 59, velocity: 1. }),
        (1, E::On { key: 60, velocity: 0.1 }),
        (7, E::On { key: 60, velocity: 0.25 }),
        (17, E::On { key: 64, velocity: 0.5 }),
        (70, E::Controller { number: 64, value: 1. }),
        (80, E::Off { key: 60, velocity: 0. }),
        (90, E::Off { key: 64, velocity: 0. }),
        (150, E::Controller { number: 64, value: 0. }),
    ];
    let reference = render(&graph, &events, 256, 256);
    assert!(reference[..14].iter().all(|v| *v == 0.));
    assert_eq!(&reference[14..34], &[0.25; 20]);
    assert!(reference[34..300].iter().all(|v| *v == 0.75));
    assert!(reference[420..].iter().all(|v| *v == 0.));
    for block in [1, 7, 64, 129] {
        assert_eq!(render(&graph, &events, 256, block), reference);
    }
    let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
    assert_eq!(render(&restored, &events, 256, 7), reference);
}
#[test]
fn stealing_retrigger_and_old_note_off_do_not_release_new_key() {
    let graph = instrument(1, false);
    let events = [
        (0, E::On { key: 60, velocity: 0.25 }),
        (80, E::On { key: 62, velocity: 0.75 }),
        (81, E::Off { key: 60, velocity: 0. }),
        (180, E::On { key: 62, velocity: 0.5 }),
    ];
    let audio = render(&graph, &events, 280, 7);
    assert_eq!(audio[160], 0.25);
    assert!(audio[300..360].iter().all(|v| *v == 0.75));
    assert!(audio[500..].iter().all(|v| *v == 0.5));
    assert!(instrument(0, false).validate().is_err());
    assert!(instrument(33, false).validate().is_err());
}
#[test]
fn component_inputs_and_private_state_roundtrip() {
    let mut body = DeviceGraph::new("org.test.scale-body", "Scale body", DeviceRole::Effect);
    body.nodes = vec![
        N::audio_input("in"),
        N::new("amount", K::ComponentInput { index: 1 }),
        N::new("mul", K::Multiply),
        N::audio_output("out"),
    ];
    body.connections =
        vec![C::ports("in", "out", "mul", "a"), C::ports("amount", "out", "mul", "b"), C::new("mul", "out")];
    let mut graph = DeviceGraph::new("org.test.component", "Component", DeviceRole::Effect);
    graph.nodes = vec![
        N::audio_input("in"),
        constant("amount", 0.5),
        N::new("component", K::Component { name: "Scale".into(), version: 1, graph: body.to_sd().unwrap() }),
        N::audio_output("out"),
    ];
    graph.connections =
        vec![C::new("in", "component"), C::ports("amount", "out", "component", "control1"), C::new("component", "out")];
    let graph = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
    for frames in [1, 7, 64, 129] {
        let mut prepared = graph.compile_at(48000.).unwrap();
        let mut output = vec![0.25; 600];
        for block in output.chunks_mut(frames * 2) {
            prepared.process(block);
        }
        assert!(output.iter().all(|v| *v == 0.125));
    }
}

#[test]
fn midi_effect_reuses_ranges_sustain_and_transpose_with_safe_overflow() {
    let mut g = DeviceGraph::new("org.test.event-chain", "Event chain", DeviceRole::MidiEffect);
    g.nodes = vec![
        N::new("in", K::MidiInput),
        N::new("keys", K::KeyRange { min: 60, max: 72 }),
        N::new("vel", K::VelocityRange { min: 0.2, max: 1. }),
        N::new("transpose", K::MidiTranspose { semitones: 12 }),
        N::new("pedal", K::Sustain),
        N::new("out", K::MidiOutput),
    ];
    g.connections = vec![
        C::new("in", "keys"),
        C::new("keys", "vel"),
        C::new("vel", "transpose"),
        C::new("transpose", "pedal"),
        C::new("pedal", "out"),
    ];
    let mut p = g.compile_midi().unwrap();
    let input = [
        (0, E::On { key: 59, velocity: 1. }),
        (1, E::On { key: 60, velocity: 0.5 }),
        (3, E::Controller { number: 64, value: 1. }),
        (4, E::Off { key: 60, velocity: 0. }),
        (9, E::Controller { number: 64, value: 0. }),
    ];
    let mut out = [(0, E::AllNotesOff); 16];
    let n = p.process(&input, &mut out);
    assert_eq!(
        &out[..n],
        &[
            (1, E::On { key: 72, velocity: 0.5 }),
            (3, E::Controller { number: 64, value: 1. }),
            (9, E::Controller { number: 64, value: 0. }),
            (9, E::Off { key: 72, velocity: 0. })
        ]
    );
    p.process(&input[..4], &mut out);
    let mut short = [(0, E::AllNotesOff); 1];
    assert_eq!(p.process(&input[4..], &mut short), 1);
    assert_eq!(short, [(9, E::AllNotesOff)]);
}
#[test]
fn named_auxiliary_outputs_inside_feedback_follow_the_frame_reference() {
    let mut g = DeviceGraph::new("org.test.aux-feedback", "Auxiliary feedback", DeviceRole::Instrument);
    g.nodes = vec![
        N::new("voice", K::VoiceInput { controller: 1 }),
        N::new("env", K::Ad),
        N::new("delay", K::SampleDelay),
        N::new("sum", K::Add),
        N::audio_output("out"),
    ];
    g.connections = vec![
        C::ports("env", "finished", "delay", "in"),
        C::ports("delay", "out", "env", "trigger"),
        C::ports("voice", "velocity", "sum", "a"),
        C::ports("env", "out", "sum", "b"),
        C::new("sum", "out"),
    ];
    let events = [(3, E::On { key: 60, velocity: 0.5 })];
    assert_eq!(render(&g, &events, 256, 1), render(&g, &events, 256, 129));
}

#[test]
fn global_controller_bend_and_pressure_sources_preserve_event_offsets() {
    let mut g = DeviceGraph::new("org.test.controls", "Controls", DeviceRole::Instrument);
    g.nodes = vec![
        N::new("cc", K::MidiController { number: 1 }),
        N::new("bend", K::PitchBend),
        N::new("pressure", K::Pressure),
        N::new("a", K::Add),
        N::new("b", K::Add),
        N::audio_output("out"),
    ];
    g.connections = vec![
        C::ports("cc", "out", "a", "a"),
        C::ports("bend", "out", "a", "b"),
        C::ports("a", "out", "b", "a"),
        C::ports("pressure", "out", "b", "b"),
        C::new("b", "out"),
    ];
    let out = render(
        &g,
        &[
            (3, E::Controller { number: 1, value: 0.25 }),
            (4, E::PitchBend(-0.5)),
            (5, E::Pressure(0.75)),
            (6, E::Pressure(f32::NAN)),
        ],
        8,
        7,
    );
    assert_eq!(out, [0., 0., 0., 0., 0., 0., 0.25, 0.25, -0.25, -0.25, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5]);
}
