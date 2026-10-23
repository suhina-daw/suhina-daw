// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_sdk::{
    Connection as C, DeviceGraph, DeviceRole, NODE_CATALOG, Node as N, NodeKind as K, Parameter, ParameterControl,
    ProcessContext, Waveform,
};
#[path = "common/mod.rs"]
mod examples;

fn graph(kind: K) -> DeviceGraph {
    if matches!(kind, K::MidiTranspose { .. } | K::MidiTransposeParameter { .. }) {
        let mut g = DeviceGraph::new("org.test.graph", "Graph", DeviceRole::MidiEffect);
        if let K::MidiTransposeParameter { parameter } = &kind {
            let mut parameter = Parameter::new(parameter, "Semitones", (-24.0, 24.0), 12.0);
            parameter.control = ParameterControl::Integer;
            g.parameters.push(parameter);
        }
        g.nodes = vec![N::new("in", K::MidiInput), N::new("node", kind), N::new("out", K::MidiOutput)];
        g.connections = vec![C::new("in", "node"), C::new("node", "out")];
        return g;
    }
    if matches!(kind, K::MidiKey | K::MidiVelocity | K::MidiGate) {
        let mut g = DeviceGraph::new("org.test.graph", "Graph", DeviceRole::Instrument);
        g.nodes = vec![N::new("node", kind), N::audio_output("out")];
        g.connections = vec![C::new("node", "out")];
        return g;
    }
    let mut g = DeviceGraph::new("org.test.graph", "Graph", DeviceRole::Effect);
    g.parameters.push(Parameter::new("amount", "Amount", (0.0, 2.0), 0.5));
    g.nodes = vec![N::audio_input("in"), N::new("node", kind), N::audio_output("out")];
    let ports = g.nodes[1].kind.info().inputs;
    for port in ports.iter().filter(|port| port.default.is_none()) {
        g.connections.push(C::ports("in", "out", "node", port.name));
    }
    if ports.iter().all(|port| port.default.is_some()) {
        g.nodes.push(N::new("sum", K::Add));
        g.connections.extend([
            C::ports("in", "out", "sum", "a"),
            C::ports("node", "out", "sum", "b"),
            C::new("sum", "out"),
        ]);
    } else {
        g.connections.push(C::new("node", "out"));
    }
    g
}

#[test]
fn midi_transpose_parameter_updates_without_recompiling() {
    use suhina_device_api::NoteEvent;
    let g = graph(K::MidiTransposeParameter { parameter: "amount".into() });
    let mut effect = g.compile_midi().unwrap();
    let input = [(0, NoteEvent::On { key: 60, velocity: 0.8 })];
    let mut output = [(0, NoteEvent::Off { key: 0, velocity: 0.0 })];
    assert_eq!(effect.process(&input, &mut output), 1);
    assert_eq!(output[0].1, NoteEvent::On { key: 72, velocity: 0.8 });
    effect.set_parameter(0, -80.0);
    assert_eq!(effect.process(&[(17, NoteEvent::Off { key: 60, velocity: 0.3 })], &mut output), 1);
    assert_eq!(output[0], (17, NoteEvent::Off { key: 72, velocity: 0.3 }));
    assert_eq!(effect.process(&input, &mut output), 1);
    assert_eq!(output[0].1, NoteEvent::On { key: 36, velocity: 0.8 });
    effect.set_parameter(0, 12.0);
    assert_eq!(effect.process(&[(0, NoteEvent::On { key: 120, velocity: 0.8 })], &mut output), 0);
    effect.set_parameter(0, 0.0);
    assert_eq!(effect.process(&[(1, NoteEvent::Off { key: 120, velocity: 0.3 })], &mut output), 0);
}

#[test]
fn every_catalog_node_round_trips_resets_and_ignores_host_block_size() {
    let mut kinds = NODE_CATALOG
        .iter()
        .filter(|info| !matches!(info.kind, "audio_input" | "audio_output" | "midi_input" | "midi_output"))
        .map(|info| serde_json::from_str::<K>(info.example).unwrap())
        .collect::<Vec<_>>();
    kinds.extend([
        K::Envelope { rms: true },
        K::Oscillator { shape: Waveform::Random },
        K::Filter { mode: suhina_device_sdk::FilterMode::Highpass },
        K::Filter { mode: suhina_device_sdk::FilterMode::Bandpass },
        K::Filter { mode: suhina_device_sdk::FilterMode::Peak },
        K::Filter { mode: suhina_device_sdk::FilterMode::LowShelf },
        K::Filter { mode: suhina_device_sdk::FilterMode::HighShelf },
    ]);
    for kind in kinds {
        let g = graph(kind.clone());
        let restored = DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap();
        assert_eq!(g, restored);
        if g.role == DeviceRole::MidiEffect {
            assert!(g.compile_midi().is_ok());
            continue;
        }
        if g.role == DeviceRole::Instrument {
            assert!(g.compile_instrument_at(48_000.0).is_ok());
            continue;
        }
        for sr in [8000.0, 44100.0, 48000.0, 96000.0, 384000.0] {
            let mut effect = g.compile_at(sr).unwrap();
            let input = (0..2048).flat_map(|n| [(n as f32 * 0.03).sin(), -(n as f32 * 0.05).cos()]).collect::<Vec<_>>();
            let mut expected = input.clone();
            effect.process(&mut expected);
            assert!(expected.iter().all(|v| v.is_finite()), "{kind:?}");
            for frames in [1, 7, 31, 512] {
                effect.reset();
                let mut output = input.clone();
                for block in output.chunks_mut(frames * 2) {
                    effect.process(block);
                }
                assert_eq!(output, expected, "{kind:?}: {sr}, {frames}");
            }
            effect.set_parameter(0, f64::NAN);
            effect.set_parameter(99, f64::INFINITY);
            let mut hostile = [f32::NAN, f32::INFINITY, f32::MAX, -f32::MAX, 1.0];
            effect.process(&mut hostile);
            assert!(hostile.iter().all(|v| v.is_finite()), "{kind:?}");
            assert_eq!(hostile[4], 0.0);
        }
    }
}

fn feedback() -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.echo", "Echo", DeviceRole::Effect);
    g.parameters.push(Parameter::new("feedback", "Feedback", (0.0, 0.9), 0.5));
    g.nodes = vec![
        N::audio_input("in"),
        N::new("sum", K::Add),
        N::new("delay", K::SampleDelay),
        N::gain("feedback", "feedback"),
        N::audio_output("out"),
    ];
    g.connections = vec![
        C::ports("in", "out", "sum", "a"),
        C::ports("feedback", "out", "sum", "b"),
        C::new("sum", "delay"),
        C::new("delay", "feedback"),
        C::new("sum", "out"),
    ];
    g
}

#[test]
fn feedback_impulse_matches_golden_and_node_storage_order_is_irrelevant() {
    let mut g = feedback();
    let bytes = include_bytes!("golden/feedback.f32");
    let expected = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect::<Vec<_>>();
    for _ in 0..g.nodes.len() {
        let mut effect = g.compile().unwrap();
        let mut output = vec![0.0; expected.len()];
        output[0] = 1.0;
        output[1] = -1.0;
        effect.process(&mut output);
        assert_eq!(output, expected);
        g.nodes.rotate_left(1);
    }
}

#[test]
fn fractional_delay_has_the_expected_two_impulse_taps() {
    let mut g = graph(K::Delay { max_seconds: 0.01 });
    g.nodes.push(N::new("time", K::Constant { value: 1.5 / 48000.0 }));
    g.connections.push(C::ports("time", "out", "node", "time"));
    let mut out = [1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    g.compile().unwrap().process(&mut out);
    assert_eq!(out, [0.0, 0.0, 0.5, -0.5, 0.5, -0.5, 0.0, 0.0]);
}

#[test]
fn branching_channel_ports_and_parameter_changes_work() {
    let mut g = graph(K::Stereo);
    g.connections =
        vec![C::ports("in", "right", "node", "left"), C::ports("in", "left", "node", "right"), C::new("node", "out")];
    let mut out = [1.0, 2.0];
    g.compile().unwrap().process(&mut out);
    assert_eq!(out, [2.0, 1.0]);
    let mut g = graph(K::Multiply);
    g.nodes.push(N::new("amount", K::Parameter { parameter: "amount".into() }));
    g.connections.retain(|e| e.to_port != "b");
    g.connections.push(C::ports("amount", "out", "node", "b"));
    let mut effect = g.compile().unwrap();
    let mut out = [1.0, -1.0];
    effect.process(&mut out);
    assert_eq!(out, [0.5, -0.5]);
    effect.set_parameter(0, 1.5);
    effect.process(&mut vec![0.0; 1920]);
    let mut out = [1.0, -1.0];
    effect.process(&mut out);
    assert_eq!(out, [1.5, -1.5]);
}

#[test]
fn invalid_ports_cycles_orphans_and_memory_requests_have_actionable_errors() {
    let mut g = feedback();
    g.nodes[2].kind = K::Negate;
    assert!(g.validate().unwrap_err().to_string().contains("cycle"));
    let mut g = feedback();
    g.connections[0].to_port = "typo".into();
    assert!(g.validate().unwrap_err().to_string().contains("sum"));
    let mut g = feedback();
    g.connections.push(g.connections[0].clone());
    assert!(g.validate().unwrap_err().to_string().contains("multiple connections"));
    let mut g = feedback();
    g.nodes.push(N::new("orphan", K::Constant { value: 1.0 }));
    assert!(g.validate().unwrap_err().to_string().contains("orphan"));
    let mut g = graph(K::Delay { max_seconds: 60.0 });
    assert!(g.validate().unwrap_err().to_string().contains("history"));
    g.nodes[1].kind = K::Delay { max_seconds: f64::INFINITY };
    assert!(g.validate().is_err());
    let mut g = graph(K::Delay { max_seconds: 0.01 });
    g.connections.push(C::ports("out", "out", "node", "time"));
    assert!(g.validate().is_err());
    let mut g = graph(K::Delay { max_seconds: 0.01 });
    g.connections.push(C::ports("node", "out", "node", "time"));
    assert!(g.validate().unwrap_err().to_string().contains("cycle"));
}

#[test]
fn version_one_gain_example_remains_readable_and_new_nodes_require_version_two() {
    let g = DeviceGraph::from_sd(include_str!("fixtures/gain-v1.sd")).unwrap();
    let mut out = [1.0, -1.0];
    g.compile().unwrap().process(&mut out);
    assert_eq!(out, [1.0, -1.0]);
    let text = graph(K::Tanh).to_sd().unwrap().replace("\"version\": 5", "\"version\": 1");
    assert!(DeviceGraph::from_sd(&text).is_err());
    assert!(g.compile_at(f64::NAN).is_err());
    assert!(g.compile_at(1e9).is_err());
}

#[test]
fn discrete_controls_are_validated_and_clamped_consistently() {
    let mut g = graph(K::Gain { parameter: "amount".into() });
    let p = &mut g.parameters[0];
    p.control = ParameterControl::Integer;
    p.default = 1.0;
    assert_eq!(p.clamp(1.7), 2.0);
    assert!(g.validate().is_ok());
    g.parameters[0].default = 0.5;
    assert!(g.validate().is_err());
    g.parameters[0].control = ParameterControl::LogSlider;
    assert!(g.validate().is_err());
    g.parameters[0].control = ParameterControl::Toggle;
    g.parameters[0].max = 1.0;
    g.parameters[0].default = 0.0;
    assert!(g.validate().is_ok());
    assert_eq!(g.parameters[0].clamp(0.7), 1.0);
}

#[test]
fn synced_lfo_follows_transport_position() {
    let mut g = graph(K::SyncedLfo { shape: Waveform::Sine });
    let mut effect = g.compile().unwrap();
    let context = ProcessContext { tempo: 120.0, beat: 0.25, playing: false };
    let mut out = [0.0; 4];
    effect.process_context(&context, &mut out);
    assert_eq!(out, [1.0; 4]);
    g.nodes[1].kind = K::Beat;
    let mut out = [0.0; 4];
    g.compile_at(8000.0).unwrap().process_context(&ProcessContext { beat: 1.0, playing: true, ..context }, &mut out);
    assert_eq!(out[0], 1.0);
    assert!((out[2] - 1.00025).abs() < 1e-6);
}

#[test]
fn six_authoring_examples_save_restore_and_have_the_expected_sound() {
    for name in ["stereo-utility", "auto-pan", "filter", "distortion", "delay", "compressor"] {
        let g = examples::build(name).unwrap();
        assert_eq!(DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap(), g);
        let mut effect = g.compile().unwrap();
        let mut out = vec![0.5; 48000];
        effect.process(&mut out);
        assert!(out.iter().all(|v| v.is_finite()));
        match name {
            "stereo-utility" => assert_eq!(out, vec![0.5; 48000]),
            "auto-pan" => {
                assert!((out[24000] - 0.0).abs() < 1e-5);
                assert!((out[24001] - 0.5).abs() < 1e-5);
            }
            "filter" => assert!((out[47998] - 0.5).abs() < 1e-4),
            "distortion" => assert!((out[47998] - (0.25 + 0.5 * 1.0f32.tanh())).abs() < 1e-6),
            "delay" => {
                assert_eq!(out[0], 0.25);
                assert!(out[24000] > 0.0 && out[24000] < 0.5);
            }
            "compressor" => assert!(out[47998] > 0.1 && out[47998] < 0.2, "{}", out[47998]),
            _ => {}
        }
    }
}

#[test]
fn every_node_has_documented_ports_and_a_valid_catalog_example() {
    let docs = include_str!("../../../docs/spec/sdk-nodes.md").replace("\r\n", "\n");
    let table =
        docs.split("<!-- sdk-nodes:begin -->\n").nth(1).unwrap().split("<!-- sdk-nodes:end -->").next().unwrap();
    assert_eq!(table, suhina_device_sdk::node_reference_markdown());
    for info in NODE_CATALOG {
        let kind: K = serde_json::from_str(info.example).unwrap();
        assert_eq!(kind.info().kind, info.kind);
        let row = docs.lines().find(|line| line.starts_with(&format!("| `{}` /", info.kind))).unwrap();
        assert!(row.contains(info.description), "stale description for {}", info.kind);
        for port in info.inputs {
            assert!(row.contains(&format!("`{}`", port.name)), "{}", info.kind);
        }
    }
}

#[test]
fn parameter_changes_are_ramped_without_steps_and_independent_of_block_size() {
    let g = graph(K::Gain { parameter: "amount".into() });
    for rate in [8000.0, 48000.0, 96000.0] {
        let mut effect = g.compile_at(rate).unwrap();
        effect.process(&mut [1.0; 2]);
        effect.set_parameter(0, 1.5);
        let frames = (rate * 0.02) as usize;
        let mut expected = vec![1.0; frames * 2];
        effect.process(&mut expected);
        assert!(expected[0] > 0.5 && expected[0] < 0.52);
        assert_eq!(*expected.last().unwrap(), 1.5);
        assert!(expected.windows(2).all(|w| (w[1] - w[0]).abs() < 0.01));
        let mut effect = g.compile_at(rate).unwrap();
        effect.process(&mut [1.0; 2]);
        effect.set_parameter(0, 1.5);
        let mut output = vec![1.0; frames * 2];
        for block in output.chunks_mut(14) {
            effect.process(block);
        }
        assert_eq!(output, expected);
    }
}

#[test]
fn customized_controls_round_trip_and_reject_invalid_choice_metadata() {
    use suhina_device_sdk::ParameterControl::*;
    for control in [Slider, LogSlider, Integer, Toggle, Knob, LogKnob, Number, VerticalSlider, Button, Choice, Radio] {
        let mut g = graph(K::Gain { parameter: "amount".into() });
        let p = &mut g.parameters[0];
        p.control = control;
        p.default = 1.0;
        if matches!(control, LogSlider | LogKnob) {
            p.min = 0.01;
        }
        if matches!(control, Toggle | Button) {
            p.max = 1.0;
        }
        if matches!(control, Choice | Radio) {
            p.ui.options = vec!["Off".into(), "Unity".into(), "Boost".into()];
        }
        p.ui.tooltip = "Output gain".into();
        p.ui.group = "Output".into();
        p.ui.decimals = 2;
        let text = g.to_sd().unwrap();
        assert_eq!(DeviceGraph::from_sd(&text).unwrap(), g);
        g.parameters[0].ui.decimals = 100;
        assert!(g.validate().is_err());
    }
}

#[test]
fn visual_ui_settings_round_trip_and_hostile_geometry_is_rejected() {
    use suhina_device_sdk::{ControlShape, LabelPosition};
    let mut g = graph(K::Gain { parameter: "amount".into() });
    let ui = &mut g.parameters[0].ui;
    ui.size = Some([240.0, 120.0]);
    ui.position = Some([300.0, 20.0]);
    ui.accent = Some([20, 190, 130]);
    ui.background = Some([12, 20, 25]);
    ui.text_color = Some([240, 240, 240]);
    ui.font_size = 18.0;
    ui.monospace = true;
    ui.shape = ControlShape::Square;
    ui.label_position = LabelPosition::Top;
    ui.show_value = false;
    assert_eq!(DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap(), g);
    for position in [[f32::NAN, 0.0], [-1.0, 0.0], [9000.0, 0.0]] {
        let mut invalid = g.clone();
        invalid.parameters[0].ui.position = Some(position);
        assert!(invalid.validate().is_err());
    }
    for size in [[0.0, 100.0], [240.0, 24.0], [f32::INFINITY, 100.0], [240.0, 9000.0]] {
        let mut invalid = g.clone();
        invalid.parameters[0].ui.size = Some(size);
        assert!(invalid.validate().is_err());
    }
    g.parameters[0].ui.font_size = 100.0;
    assert!(g.validate().is_err());
}

#[test]
fn compact_top_label_files_remain_readable() {
    let mut g = graph(K::Gain { parameter: "amount".into() });
    let ui = &mut g.parameters[0].ui;
    ui.font_size = 8.0;
    ui.label_position = suhina_device_sdk::LabelPosition::Top;
    ui.size = Some([48.0, 44.0]);
    assert_eq!(DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap(), g);
}

#[test]
fn delay_panel_has_working_stereo_timing_ping_pong_and_valid_selectors() {
    let mut g = examples::build("delay").unwrap();
    assert_eq!(g, DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap());
    for p in &mut g.parameters {
        p.default = match p.id.as_str() {
            "filter_on" | "sync_left" | "sync_right" | "link" => 0.0,
            "time_left" => 10.0,
            "time_right" => 20.0,
            "offset_left" => 33.3,
            "offset_right" => -33.3,
            "wet" => 100.0,
            "mode" => 2.0,
            "ping_pong" => 1.0,
            _ => p.default,
        };
    }
    let mut audio = vec![0.0; 800];
    audio[0] = 1.0;
    g.compile_at(8000.0).unwrap().process(&mut audio);
    assert!((audio[160] - 1.0).abs() < 1e-5, "first repeat in left channel");
    assert!((audio[481] - 0.5).abs() < 1e-5, "second repeat in right channel");
    assert_eq!(audio[161], 0.0);
    assert_eq!(audio[480], 0.0);
    let canvas = g.ui.as_mut().unwrap();
    let selector =
        canvas.controls.iter_mut().find(|c| matches!(c.gesture, suhina_device_sdk::ui::UiGesture::Set(_))).unwrap();
    selector.gesture = suhina_device_sdk::ui::UiGesture::Set(999);
    assert!(g.validate().is_err());
}

#[test]
fn logarithmic_delay_curve_is_smooth_when_narrow_and_handles_bad_math() {
    use suhina_device_sdk::ui::{Draw, UiValue};
    assert!((UiValue::from(2.0).ln().exp().evaluate(&|_, _, _| 0.0) - 2.0).abs() < 1e-5);
    assert_eq!(UiValue::from(-1.0).ln().evaluate(&|_, _, _| 0.0), 0.0);
    assert_eq!(UiValue::from(1000.0).exp().evaluate(&|_, _, _| 0.0), 0.0);
    let g = examples::build("delay").unwrap();
    let control = g.ui.as_ref().unwrap().controls.iter().find(|c| c.id == "filter_graph").unwrap();
    let drawings = control.drawings();
    let points = drawings
        .iter()
        .find_map(|d| match d {
            Draw::Path { points, .. } => Some(points),
            _ => None,
        })
        .unwrap();
    for (cutoff, width) in [(4010.0, 2.4), (2600.0, 3.09), (1000.0, 8.0)] {
        let lookup = |id: &str, norm: bool, log: bool| {
            let p = g.parameters.iter().find(|p| p.id == id).unwrap();
            let v = match id {
                "cutoff" => cutoff,
                "width" => width,
                _ => p.default,
            };
            if !norm {
                v
            } else if log {
                (v.ln() - p.min.ln()) / (p.max.ln() - p.min.ln())
            } else {
                (v - p.min) / (p.max - p.min)
            }
        };
        let ys: Vec<_> = points.iter().map(|p| p[1].evaluate(&lookup) * 118. / control.bounds[3]).collect();
        assert!(ys.iter().all(|y| y.is_finite()));
        let peak = ys.iter().copied().fold(f32::INFINITY, f32::min);
        assert!((30.0..45.0).contains(&peak));
        for pair in ys.windows(2) {
            if pair[0] < 118. && pair[1] < 118. {
                assert!((pair[1] - pair[0]).abs() < 9.0, "abrupt visible edge: {pair:?}");
            }
        }
        if width < 4.0 {
            assert!(ys[0] > 118.0 && *ys.last().unwrap() > 60.0);
        }
    }
}

#[test]
fn lfo_rate_modes_convert_real_units_and_follow_tempo() {
    for (mode, hz) in [(0., 0.5), (1., 0.5), (2., 2.), (3., 3.), (4., 4. / 3.), (5., 2.)] {
        let mut g = graph(K::LfoRate);
        g.nodes.push(N::new("mode", K::Constant { value: mode }));
        g.connections.push(C::ports("mode", "out", "node", "mode"));
        let mut audio = [0.; 2];
        g.compile().unwrap().process(&mut audio);
        assert!((audio[0] as f64 - hz).abs() < 1e-6, "mode {mode}");
        assert_eq!(audio[0], audio[1]);
        if mode >= 2. {
            g.nodes.push(N::new("tempo", K::Tempo));
            g.connections.push(C::ports("tempo", "out", "node", "tempo"));
            let mut effect = g.compile().unwrap();
            audio.fill(0.0);
            effect.process_context(&ProcessContext { tempo: 90., beat: 0., playing: true }, &mut audio);
            assert!((audio[0] as f64 - hz * 0.75).abs() < 1e-6, "tempo in mode {mode}");
        }
    }
    for (mode, hz) in [(2., 4.), (3., 6.), (4., 8. / 3.), (5., 1.)] {
        let mut g = graph(K::LfoRate);
        for (id, value, port) in [("mode", mode, "mode"), ("division", 8., "division")] {
            g.nodes.push(N::new(id, K::Constant { value }));
            g.connections.push(C::ports(id, "out", "node", port));
        }
        let mut audio = [0.; 2];
        g.compile().unwrap().process(&mut audio);
        assert!((audio[0] as f64 - hz).abs() < 1e-6);
    }
    let mut bad = examples::build("delay").unwrap();
    bad.parameters.iter_mut().find(|p| p.id == "mod_shape").unwrap().control = ParameterControl::Integer;
    assert!(bad.validate().is_err(), "menu requires declared Choice options");
}

#[test]
fn lfo_disclosure_round_trips_without_changing_audio() {
    let graph = examples::build("delay").unwrap();
    let index = graph.parameters.iter().position(|p| p.id == "lfo_expanded").unwrap();
    let mut collapsed = graph.clone();
    collapsed.parameters[index].default = 0.0;
    assert_eq!(collapsed, DeviceGraph::from_sd(&collapsed.to_sd().unwrap()).unwrap());
    let mut open_effect = graph.compile().unwrap();
    let mut folded_effect = collapsed.compile().unwrap();
    let mut open: Vec<_> = (0..96000).map(|i| ((i as f32) * 0.02).sin() * 0.1).collect();
    let mut folded = open.clone();
    open_effect.process(&mut open);
    folded_effect.process(&mut folded);
    assert_eq!(open, folded);
    open_effect.set_parameter(index, 0.0);
    let mut next = vec![0.1; 2048];
    let mut reference = next.clone();
    open_effect.process(&mut next);
    folded_effect.process(&mut reference);
    assert_eq!(next, reference);
}

#[test]
fn effect_examples_use_circular_knobs_and_square_value_controls() {
    use suhina_device_sdk::ControlShape;

    for name in ["auto-pan", "compressor", "delay", "distortion", "filter", "stereo-utility"] {
        let graph = examples::build(name).unwrap();
        for parameter in &graph.parameters {
            let expected = if matches!(parameter.control, ParameterControl::Knob | ParameterControl::LogKnob) {
                ControlShape::Rounded
            } else {
                ControlShape::Square
            };
            assert_eq!(parameter.ui.shape, expected, "{name}: {}", parameter.id);
        }
        if name == "delay" {
            let canvas = graph.ui.as_ref().unwrap();
            assert!(canvas.layers.is_empty());
            assert!(canvas.controls.iter().all(|control| control.widget.is_some() && control.drawing.is_empty()));
        }
    }
}

#[test]
fn delay_layout_has_no_overlapping_controls_in_any_timing_or_motion_mode() {
    let graph = examples::build("delay").unwrap();
    let canvas = graph.ui.as_ref().unwrap();
    assert!(canvas.size[1] + 32. <= 200.);
    for expanded in [0., 1.] {
        for left in [0., 1.] {
            for right in [0., 1.] {
                for mode in 0..6 {
                    let lookup = |id: &str, _: bool, _: bool| match id {
                        "lfo_expanded" => expanded,
                        "sync_left" => left,
                        "sync_right" => right,
                        "mod_rate_mode" => mode as f64,
                        _ => 0.,
                    };
                    let visible: Vec<_> = canvas
                        .controls
                        .iter()
                        .filter(|c| c.visible_when.as_ref().is_none_or(|v| v.evaluate(&lookup) != 0.))
                        .collect();
                    assert_eq!(visible.iter().filter(|c| c.id == "mod_shape").count(), 1);
                    assert_eq!(visible.iter().filter(|c| c.id == "filter_graph").count(), 1);
                    for (i, a) in visible.iter().enumerate() {
                        let [x, y, w, h] = a.bounds;
                        assert!(x >= 0. && y >= 0. && x + w <= canvas.size[0] && y + h <= canvas.size[1]);
                        for b in &visible[i + 1..] {
                            let [bx, by, bw, bh] = b.bounds;
                            assert!(
                                x + w <= bx || bx + bw <= x || y + h <= by || by + bh <= y,
                                "{} overlaps {}",
                                a.id,
                                b.id
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn delay_dials_share_centres_despite_different_caption_lengths_and_widths() {
    use suhina_device_sdk::ui::Draw;
    let graph = examples::build("delay").unwrap();
    let canvas = graph.ui.as_ref().unwrap();
    let centre = |id: &str| {
        let c = canvas.controls.iter().find(|c| c.id == id).unwrap();
        c.drawings()
            .iter()
            .find_map(|draw| match draw {
                Draw::Arc { center, .. } => Some(c.bounds[1] + center[1].evaluate(&|_, _, _| 0.)),
                _ => None,
            })
            .unwrap()
    };
    for id in ["mod_filter", "mod_morph"] {
        assert_eq!(centre(id), centre("mod_delay"), "{id}");
    }
    assert_eq!(centre("feedback"), centre("wet"));
}

#[test]
fn delay_has_one_time_control_no_stereo_mirrors_and_explicit_rhythm_values() {
    use suhina_device_sdk::ui::Widget;
    let g = examples::build("delay").unwrap();
    let ui = g.ui.as_ref().unwrap();
    assert!(!ui.controls.iter().any(|c| c.id == "STEREO"));
    assert_eq!(ui.controls.iter().filter(|c| c.y.as_ref().is_some_and(|b| b.parameter == "mod_time")).count(), 1);
    let mix = ui.controls.iter().find(|c| c.id == "wet").unwrap();
    assert!(ui.controls.iter().filter(|c| c.y.is_some() && c.id != "wet").all(|c| c.bounds[0] < mix.bounds[0]));
    for (mode, expected) in [(2, "1/8"), (3, "1/8 T"), (4, "1/8 D"), (5, "8 x 1/16")] {
        let c = ui.controls.iter().find(|c| c.id == format!("motion_division_{mode}")).unwrap();
        let Some(Widget::ValueMenu { choices }) = &c.widget else { panic!("missing rhythm menu") };
        assert_eq!(choices.iter().find(|(v, _)| *v == 8).unwrap().1, expected);
    }
    let mut bad = g.clone();
    let c = bad
        .ui
        .as_mut()
        .unwrap()
        .controls
        .iter_mut()
        .find(|c| matches!(c.widget, Some(Widget::ValueMenu { .. })))
        .unwrap();
    c.widget = Some(Widget::ValueMenu { choices: vec![(0, "bad".into())] });
    assert!(bad.validate().is_err());
}

#[test]
fn delay_uses_independent_widgets_and_rejects_invalid_widget_contracts() {
    use suhina_device_sdk::ui::{Draw, Widget};
    let graph = examples::build("delay").unwrap();
    let canvas = graph.ui.as_ref().unwrap();
    assert!(canvas.controls.iter().all(|c| c.widget.is_some() && c.drawing.is_empty()));
    let encoded = graph.to_sd().unwrap();
    assert!(encoded.contains("\"widget\""));
    assert_eq!(graph, DeviceGraph::from_sd(&encoded).unwrap());
    let mut bad = graph.clone();
    let control = &mut bad.ui.as_mut().unwrap().controls[0];
    control.drawing.push(Draw::Rect { rect: [0., 0., 8., 8.].map(Into::into), color: [0; 4], radius: 0. });
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].widget = Some(Widget::Menu);
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    let control = bad
        .ui
        .as_mut()
        .unwrap()
        .controls
        .iter_mut()
        .find(|c| matches!(c.widget, Some(Widget::FilterBand { .. })))
        .unwrap();
    control.widget = Some(Widget::FilterBand { min_hz: 0., max_hz: 18000. });
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    let control =
        bad.ui.as_mut().unwrap().controls.iter_mut().find(|c| matches!(c.widget, Some(Widget::Number { .. }))).unwrap();
    if let Some(Widget::Number { decimals, .. }) = &mut control.widget {
        *decimals = 99;
    }
    assert!(bad.validate().is_err());
}

#[test]
fn delay_controls_have_padding_frames_and_aligned_centres() {
    use suhina_device_sdk::ui::Draw;
    let graph = examples::build("delay").unwrap();
    let canvas = graph.ui.as_ref().unwrap();
    let control = |id: &str| canvas.controls.iter().find(|c| c.id == id).unwrap();
    let lookup = |_: &str, _: bool, _: bool| 0.;
    let link = control("link_99");
    let timing = control("division_left");
    assert_eq!(link.bounds[1] + link.bounds[3] / 2., timing.bounds[1] + timing.bounds[3] / 2.);
    assert!(
        canvas
            .controls
            .iter()
            .filter_map(|c| c.widget.as_ref())
            .all(|w| !matches!(w, suhina_device_sdk::ui::Widget::Number { bordered: false, .. }))
    );
    let offset = control("offset_left").drawings();
    assert!(offset.iter().any(|d| matches!(d, Draw::Text { position, .. } if position[0].evaluate(&lookup) == 4.)));
    for id in ["division_left", "division_right", "mod_rate_mode_99"] {
        let c = control(id);
        let drawings = c.drawings();
        assert!(matches!(drawings[0], Draw::Rect { .. }), "{id}");
        let points = drawings
            .iter()
            .find_map(|d| match d {
                Draw::Path { points, .. } => Some(points),
                _ => None,
            })
            .unwrap();
        assert_eq!(points.last().unwrap()[0].evaluate(&lookup), c.bounds[2] - 4.);
        assert_eq!(points[0][1].evaluate(&lookup), c.bounds[3] / 2. - 2.);
    }
}

#[test]
fn midi_repress_and_zero_velocity_release_previous_mapping() {
    use suhina_device_api::NoteEvent;
    let g = graph(K::MidiTransposeParameter { parameter: "amount".into() });
    for zero_velocity in [false, true] {
        let mut effect = g.compile_midi().unwrap();
        let on = [(3, NoteEvent::On { key: 60, velocity: 0.8 })];
        let mut output = [(0, NoteEvent::AllNotesOff); 2];
        assert_eq!(effect.process(&on, &mut output), 1);
        effect.set_parameter(0, 0.0);
        if zero_velocity {
            assert_eq!(effect.process(&[(7, NoteEvent::On { key: 60, velocity: 0.0 })], &mut output), 1);
            assert_eq!(output[0], (7, NoteEvent::Off { key: 72, velocity: 0.0 }));
        } else {
            // No room for both: the sounding note ends and the press is dropped.
            assert_eq!(effect.process(&on, &mut output[..1]), 1);
            assert_eq!(output[0], (3, NoteEvent::Off { key: 72, velocity: 0.0 }));
            assert_eq!(effect.process(&on, &mut output), 1);
            assert_eq!(output[0], on[0]);
            assert_eq!(effect.process(&on, &mut output), 2);
            assert_eq!(output[0], (3, NoteEvent::Off { key: 60, velocity: 0.0 }));
            assert_eq!(output[1], on[0]);
            assert_eq!(effect.process(&[(7, NoteEvent::Off { key: 60, velocity: 0.0 })], &mut output), 1);
            assert_eq!(output[0], (7, NoteEvent::Off { key: 60, velocity: 0.0 }));
        }
        assert_eq!(effect.process(&[(8, NoteEvent::Off { key: 60, velocity: 0.0 })], &mut output), 0);
    }
}

#[test]
fn midi_release_of_a_note_from_before_the_effect_is_transposed() {
    use suhina_device_api::NoteEvent;
    let mut effect = graph(K::MidiTransposeParameter { parameter: "amount".into() }).compile_midi().unwrap();
    let mut output = [(0, NoteEvent::AllNotesOff); 2];
    // The chain was rebuilt while the key was down: the release still reaches the note.
    assert_eq!(effect.process(&[(4, NoteEvent::Off { key: 60, velocity: 0.5 })], &mut output), 1);
    assert_eq!(output[0], (4, NoteEvent::Off { key: 72, velocity: 0.5 }));
    // A key released once has nothing more to end.
    assert_eq!(effect.process(&[(5, NoteEvent::On { key: 61, velocity: 0.5 })], &mut output), 1);
    assert_eq!(effect.process(&[(6, NoteEvent::Off { key: 61, velocity: 0.5 })], &mut output), 1);
    assert_eq!(effect.process(&[(7, NoteEvent::Off { key: 61, velocity: 0.5 })], &mut output), 0);
}

#[test]
fn parameter_controls_share_discrete_semantics() {
    use ParameterControl::*;
    for control in [Integer, Toggle, Button, Choice, Radio, Slider, LogSlider, Knob, LogKnob, Number, VerticalSlider] {
        let stepped = matches!(control, Integer | Toggle | Button | Choice | Radio);
        assert_eq!(control.is_stepped(), stepped);
        let mut parameter = Parameter::new("value", "Value", (0.0, 1.0), 0.0);
        parameter.control = control;
        assert_eq!(parameter.clamp(0.7), if stepped { 1.0 } else { 0.7 });
    }
}

/// The fixture a pull request's check steps add to a track: version 5 with an expanded panel
/// whose control turns the Power toggle.
#[test]
fn the_expanded_panel_fixture_reads_and_round_trips() {
    let text = include_str!("fixtures/expanded-panel.sd");
    let g = DeviceGraph::from_sd(text).unwrap();
    let expanded = g.expanded.as_ref().unwrap();
    assert_eq!(expanded.controls[0].name, "Big power switch");
    assert!(g.ui.is_none());
    // A Windows checkout may have turned the fixture's line ends into CRLF; `to_sd` writes LF.
    assert_eq!(g.to_sd().unwrap(), text.replace("\r\n", "\n"));
}
