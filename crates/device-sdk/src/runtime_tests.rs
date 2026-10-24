// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{Connection, DeviceRole, Node};

#[test]
fn native_source_lifecycle_is_independent_of_graph_kind() {
    struct Source {
        value: f32,
        spans: usize,
    }
    impl NativeNode for Source {
        fn prepare_block(&mut self, _: &ProcessContext) {
            self.spans += 1;
        }
        fn process(&mut self, _inputs: [&[f32]; 5], buffer: &mut [f32]) {
            buffer.fill(self.value);
        }
        fn event(&mut self, _: suhina_device_api::NoteEvent) {
            self.value = 0.75;
        }
        fn reset(&mut self) {
            self.value = 0.0;
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    let mut graph = DeviceGraph::new("org.test.native", "Native", DeviceRole::Instrument);
    graph.nodes = vec![Node::new("source", NodeKind::Constant { value: 0.0 }), Node::audio_output("out")];
    graph.connections = vec![Connection::new("source", "out")];
    for blocks in [false, true] {
        let mut prepared = PreparedEffect::new(&graph, 48000.).expect("compile");
        prepared.nodes[0].state = State::Native(Box::new(Source { value: 0.25, spans: 0 }));
        prepared.block_safe = blocks;
        let mut audio = [0.0; 260];
        prepared.process_instrument_context(
            &ProcessContext::default(),
            &[(65, suhina_device_api::NoteEvent::Off { key: 60, velocity: 0.0 })],
            &mut audio,
        );
        assert!(audio[..130].iter().all(|&v| v == 0.25));
        assert!(audio[130..].iter().all(|&v| v == 0.75));
        let source =
            prepared.native_node_mut("source").expect("binding").as_any_mut().downcast_mut::<Source>().expect("type");
        assert_eq!(source.spans, 2);
        prepared.reset();
        prepared.process(&mut audio);
        assert!(audio.iter().all(|&v| v == 0.0));
        assert!(prepared.native_node_mut("missing").is_none());
    }
}

fn delayed_graph(feedback: bool) -> DeviceGraph {
    let mut graph = DeviceGraph::new("org.test.islands", "Islands", DeviceRole::Effect);
    graph.parameters.push(Parameter::new("gain", "Gain", (0.0, 1.0), 0.25));
    graph.nodes.push(Node::audio_input("in"));
    let mut source = "in".to_owned();
    for index in 0..3 {
        let sum = format!("sum{index}");
        let gain = format!("gain{index}");
        let delay = format!("delay{index}");
        graph.nodes.extend([
            Node::new(&sum, NodeKind::Add),
            Node::new(&gain, NodeKind::Gain { parameter: "gain".into() }),
            Node::new(&delay, if index == 1 { NodeKind::Delay { max_seconds: 0.01 } } else { NodeKind::SampleDelay }),
        ]);
        graph.connections.extend([
            Connection::ports(&source, "out", &sum, "a"),
            Connection::ports(if feedback { &delay } else { &source }, "out", &sum, "b"),
            Connection::new(&sum, &gain),
            Connection::new(&gain, &delay),
        ]);
        source = delay;
    }
    graph.nodes.push(Node::audio_output("out"));
    graph.connections.push(Connection::new(&source, "out"));
    graph
}

#[test]
fn feedback_islands_match_reference_across_boundaries_and_reset() {
    for topology in 0..3 {
        let feedback = topology != 0;
        let mut graph = delayed_graph(feedback);
        if topology == 2 {
            graph
                .connections
                .iter_mut()
                .find(|edge| edge.to == "sum0" && edge.to_port == "b")
                .expect("feedback edge")
                .from = "delay2".into();
        }
        let mut block = PreparedEffect::new(&graph, 48000.0).expect("graph");
        assert_eq!(block.block_safe, !feedback);
        block.block_safe = true;
        let mut reference = PreparedEffect::new(&graph, 48000.0).expect("graph");
        reference.block_safe = false;
        assert!(block.islands.iter().any(|island| island.nodes.len() == 1));
        assert_eq!(
            block.islands.iter().filter(|island| island.nodes.len() > 1).count(),
            match topology {
                0 => 0,
                1 => 3,
                _ => 1,
            }
        );
        for pass in 0..3 {
            block.set_parameter(0, 0.1 + pass as f64 * 0.1);
            reference.set_parameter(0, 0.1 + pass as f64 * 0.1);
            for size in [0, 1, 2, 126, 128, 130, 2049] {
                let mut actual: Vec<_> = (0..size)
                    .map(|i| match i % 17 {
                        0 => f32::NAN,
                        1 => f32::INFINITY,
                        _ => (i as f32 * 0.1).sin(),
                    })
                    .collect();
                let mut expected = actual.clone();
                block.process(&mut actual);
                reference.process(&mut expected);
                assert_eq!(actual, expected, "feedback={feedback}, pass={pass}, size={size}");
            }
            block.reset();
            reference.reset();
        }
    }
}

#[test]
#[ignore = "manual release callback timing"]
fn representative_graph_callback_timing() {
    for (feedback, tail) in [(false, 0), (true, 0), (true, 16)] {
        let mut graph = delayed_graph(feedback);
        graph.connections.retain(|edge| edge.to != "out");
        let mut previous = "delay2".to_owned();
        for i in 0..tail {
            let id = format!("tail{i}");
            graph.nodes.push(Node::new(&id, NodeKind::Gain { parameter: "gain".into() }));
            graph.connections.push(Connection::new(&previous, &id));
            previous = id;
        }
        graph.connections.push(Connection::new(&previous, "out"));
        for enabled in [false, true] {
            let mut effect = PreparedEffect::new(&graph, 48000.0).expect("graph");
            effect.block_safe &= enabled;
            let mut audio = [0.1; 512];
            let start = std::time::Instant::now();
            for _ in 0..10000 {
                effect.process(std::hint::black_box(&mut audio));
            }
            eprintln!(
                "three delay islands feedback={feedback} tail={tail} block={enabled}: {:?}/256 frames",
                start.elapsed() / 10000
            );
        }
    }
    for count in [2, 16, 128] {
        let mut graph = DeviceGraph::new("org.test.timing", "Timing", DeviceRole::Effect);
        graph.parameters.push(Parameter::new("gain", "Gain", (0.0, 2.0), 0.99));
        graph.nodes.push(Node::audio_input("in"));
        let mut previous = "in".to_string();
        for i in 0..count {
            let id = format!("gain{i}");
            graph.nodes.push(Node::new(&id, NodeKind::Gain { parameter: "gain".into() }));
            graph.connections.push(Connection::new(&previous, &id));
            previous = id;
        }
        graph.nodes.push(Node::audio_output("out"));
        graph.connections.push(Connection::new(&previous, "out"));
        for enabled in [false, true] {
            let mut effect = PreparedEffect::new(&graph, 48000.0).expect("graph");
            effect.block_safe = enabled;
            let mut audio = [0.1; 512];
            let start = std::time::Instant::now();
            for _ in 0..10000 {
                effect.process(std::hint::black_box(&mut audio));
            }
            eprintln!("{count} gains block={enabled}: {:?}/256 frames", start.elapsed() / 10000);
        }
    }
}

#[test]
fn blocks_match_frame_interpreter_with_smoothing_and_transport() {
    for source in [
        NodeKind::Beat,
        NodeKind::Noise { seed: 71 },
        NodeKind::MidiGate,
        NodeKind::Oscillator { shape: crate::Waveform::Sine },
    ] {
        let mut graph = DeviceGraph::new("org.test.block", "Block", DeviceRole::Effect);
        graph.parameters.push(Parameter::new("gain", "Gain", (0.0, 2.0), 0.5));
        graph.nodes = vec![
            Node::new("source", source),
            Node::new("gain", NodeKind::Gain { parameter: "gain".into() }),
            Node::audio_output("out"),
        ];
        graph.connections = vec![Connection::new("source", "gain"), Connection::new("gain", "out")];
        let mut block = PreparedEffect::new(&graph, 48000.0).expect("graph");
        let mut reference = PreparedEffect::new(&graph, 48000.0).expect("graph");
        reference.block_safe = false;
        let context = ProcessContext { tempo: 91.0, beat: 1.25, playing: true };
        for size in [0, 1, 2, 126, 128, 130, 2049] {
            let mut actual = vec![0.1; size];
            let mut expected = actual.clone();
            block.set_parameter(0, 1.5);
            reference.set_parameter(0, 1.5);
            block.process_context(&context, &mut actual);
            reference.process_context(&context, &mut expected);
            assert_eq!(actual, expected, "buffer size {size}");
        }
    }
}
#[test]
fn many_parameter_timelines_match_frame_interpreter() {
    let mut graph = DeviceGraph::new("org.test.parameters", "Parameters", DeviceRole::Effect);
    graph.nodes.push(Node::new("source", NodeKind::Beat));
    let mut previous = "source".to_owned();
    for index in 0..128 {
        let id = format!("gain{index}");
        graph.parameters.push(Parameter::new(&id, &id, (0.0, 2.0), 1.0));
        graph.nodes.push(Node::new(&id, NodeKind::Gain { parameter: id.clone() }));
        graph.connections.push(Connection::new(&previous, &id));
        previous = id;
    }
    graph.nodes.push(Node::audio_output("out"));
    graph.connections.push(Connection::new(&previous, "out"));
    let mut block = PreparedEffect::new(&graph, 48000.0).expect("graph");
    let mut reference = PreparedEffect::new(&graph, 48000.0).expect("graph");
    reference.block_safe = false;
    let context = ProcessContext { tempo: 123.0, beat: 0.5, playing: true };
    for pass in 0..3 {
        for index in 0..128 {
            let value = 0.99 + ((index + pass) % 3) as f64 * 0.01;
            block.set_parameter(index, value);
            reference.set_parameter(index, value);
        }
        let mut actual = vec![0.0; 2050];
        let mut expected = actual.clone();
        block.process_context(&context, &mut actual);
        reference.process_context(&context, &mut expected);
        assert_eq!(actual, expected, "parameter update pass {pass}");
    }
}
