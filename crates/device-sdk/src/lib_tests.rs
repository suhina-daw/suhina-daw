// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn gain_graph() -> DeviceGraph {
    let mut graph = DeviceGraph::new("org.example.gain", "Example Gain", DeviceRole::Effect);
    graph.parameters.push(Parameter::new("gain", "Gain", (0.0, 2.0), 1.0));
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
    graph
}

#[test]
fn gain_graph_round_trips_and_processes_audio() {
    let mut graph = gain_graph();
    graph.parameters[0].smoothing_ms = 0.0;
    let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(restored, graph);
    let mut effect = restored.compile().unwrap_or_else(|e| panic!("{e}"));
    effect.set_parameter(0, 0.5);
    let mut buffer = [1.0, -1.0, 0.5, -0.5];
    effect.process(&mut buffer);
    assert_eq!(buffer, [0.5, -0.5, 0.25, -0.25]);
    effect.set_parameter(0, f64::NAN);
    let mut buffer = [1.0, -1.0];
    effect.process(&mut buffer);
    assert_eq!(buffer, [1.0, -1.0]);
}

#[test]
fn device_revisions_are_separate_from_format_versions_and_old_files_default_to_one() {
    let old = DeviceGraph::from_sd(include_str!("../tests/fixtures/gain-v1.sd")).expect("legacy graph loads");
    assert_eq!(old.device_version, 1);
    let encoded = old.to_sd().expect("legacy graph upgrades to current format");
    let json: serde_json::Value = serde_json::from_str(&encoded).expect("valid JSON");
    assert_eq!(json["version"], VERSION, "every file is written as the current version");
    assert_eq!(json["device"]["version"], 1);
    assert_eq!(DeviceGraph::from_sd(&encoded), Ok(old));
}

#[test]
fn a_panel_style_sheet_round_trips_as_version_6_and_older_versions_refuse_it() {
    let mut graph = gain_graph();
    let style = "#gain { color: #ff8800; }\n";
    graph.ui = Some(ui::DeviceUi { style: Some(style.into()), ..ui::DeviceUi::default() });
    let encoded = graph.to_sd().unwrap();
    let json: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(json["version"], 6);
    assert_eq!(json["device"]["ui"]["style"], style);
    assert_eq!(DeviceGraph::from_sd(&encoded), Ok(graph.clone()));
    assert!(DeviceGraph::from_sd(&encoded.replace("\"version\": 6", "\"version\": 5")).is_err());
    // Without a style sheet the file stays version 4, and the key is not written.
    let plain = gain_graph().to_sd().unwrap();
    assert!(!plain.contains("\"style\""));
    let mut huge = graph;
    huge.ui = Some(ui::DeviceUi { style: Some("a".repeat(ui::MAX_STYLE_BYTES + 1)), ..ui::DeviceUi::default() });
    assert!(huge.validate().is_err());
}

#[test]
fn an_expanded_panel_round_trips_and_older_versions_refuse_it() {
    let mut graph = gain_graph();
    graph.ui = Some(ui::DeviceUi { size: [260., 168.], ..ui::DeviceUi::default() });
    graph.expanded = Some(ui::DeviceUi { size: [720., 400.], ..ui::DeviceUi::default() });
    let encoded = graph.to_sd().unwrap();
    let json: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(json["version"], VERSION);
    assert_eq!(json["device"]["expanded"]["size"], serde_json::json!([720.0, 400.0]));
    assert_eq!(DeviceGraph::from_sd(&encoded), Ok(graph.clone()));
    assert!(DeviceGraph::from_sd(&encoded.replace("\"version\": 6", "\"version\": 4")).is_err());
    // The expanded panel is checked against the parameters like the compact one.
    let mut bad = graph;
    bad.expanded = Some(ui::DeviceUi { hidden_parameters: vec!["missing".into()], ..ui::DeviceUi::default() });
    assert!(bad.validate().is_err());
}

#[test]
fn device_migration_renames_parameter_ids_and_clamps_saved_values() {
    let mut graph = gain_graph();
    graph.device_version = 2;
    graph.migrations.push(DeviceMigration {
        from_version: 1,
        parameter_ids: vec![ParameterIdMigration { from: "gain".into(), to: "level".into() }],
    });
    graph.parameters[0].id = "level".into();
    graph.parameters[0].max = 1.0;
    graph.nodes[1].kind = NodeKind::Gain { parameter: "level".into() };

    let encoded = graph.to_sd().expect("migration serializes");
    let loaded = DeviceGraph::from_sd(&encoded).expect("migration loads");
    assert_eq!(loaded, graph);
    assert_eq!(loaded.migrate_parameter_values(1, &[("gain".into(), 1.5)]), Ok(vec![("level".into(), 1.0)]));
    assert_eq!(loaded.migrate_parameter_values(1, &[]), Ok(vec![("level".into(), 1.0)]));
}

#[test]
fn a_device_revision_bump_requires_a_previous_revision_migration() {
    let mut graph = gain_graph();
    graph.device_version = 2;
    assert!(graph.validate().is_err());
    graph.migrations.push(DeviceMigration { from_version: 1, parameter_ids: Vec::new() });
    assert!(graph.validate().is_ok());
    assert!(graph.migrate_parameter_values(1, &[]).is_ok());
    assert!(graph.migrate_parameter_values(3, &[]).is_err());
}

#[test]
fn compiled_gains_follow_connections_regardless_of_storage_order() {
    let mut graph = DeviceGraph::new("org.example.chain", "Gain Chain", DeviceRole::Effect);
    graph.parameters =
        vec![Parameter::new("up", "Up", (0.0, 1e30), 1e30), Parameter::new("down", "Down", (0.0, 1.0), 1e-30)];
    graph.nodes = vec![
        Node::audio_input("in"),
        Node::gain("up1", "up"),
        Node::gain("up2", "up"),
        Node::gain("down1", "down"),
        Node::gain("down2", "down"),
        Node::audio_output("out"),
    ];
    graph.connections = vec![
        Connection::new("in", "up1"),
        Connection::new("up1", "down1"),
        Connection::new("down1", "up2"),
        Connection::new("up2", "down2"),
        Connection::new("down2", "out"),
    ];
    let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
    let mut effect = restored.compile().unwrap();
    let mut buffer = [1.0, -1.0];
    effect.process(&mut buffer);
    for (sample, expected) in buffer.into_iter().zip([1.0, -1.0]) {
        assert!(sample.is_finite());
        assert!((sample - expected).abs() < 1e-5, "{sample}");
    }
}

#[test]
fn hidden_nodes_round_trip_without_changing_the_graph() {
    let mut graph = gain_graph();
    graph.nodes[0].visible = false;
    graph.nodes[1].visible = false;
    graph.nodes[2].visible = false;
    let text = graph.to_sd().unwrap_or_default();
    assert_eq!(text.matches("\"visible\": false").count(), 3);
    assert_eq!(DeviceGraph::from_sd(&text).map(|restored| restored.nodes), Ok(graph.nodes));
}

#[test]
fn invalid_graphs_and_oversized_files_are_refused() {
    let mut graph = gain_graph();
    graph.connections.pop();
    assert!(graph.validate().is_err());
    let too_large = " ".repeat(MAX_FILE_BYTES + 1);
    assert!(DeviceGraph::from_sd(&too_large).is_err());
    assert!(DeviceGraph::from_sd("not json").is_err());
    assert!(DeviceGraph::from_sd(r#"{"format":"suhina-device","version":99}"#).is_err());
}

#[test]
fn malformed_parameters_and_missing_gain_parameters_are_refused() {
    let mut graph = gain_graph();
    graph.parameters[0].default = 3.0;
    assert!(graph.validate().is_err());
    graph.parameters[0].default = 1.0;
    graph.nodes[1] = Node::gain("gain", "missing");
    assert!(graph.validate().is_err());
}

#[test]
fn non_audio_roles_are_serializable_but_not_audio_effects() {
    let mut graph = DeviceGraph::new("org.test.instrument", "Instrument", DeviceRole::Instrument);
    graph.nodes = vec![Node::new("gate", NodeKind::MidiGate), Node::audio_output("out")];
    graph.connections = vec![Connection::new("gate", "out")];
    let encoded = graph.to_sd().expect("valid instrument graph serializes");
    assert_eq!(DeviceGraph::from_sd(&encoded).map(|g| g.role), Ok(DeviceRole::Instrument));
    assert!(graph.compile().is_err());
}

#[test]
fn midi_effect_transposes_events_and_preserves_frames_and_controls() {
    use suhina_device_api::NoteEvent;
    let mut graph = DeviceGraph::new("org.test.transpose", "Transpose", DeviceRole::MidiEffect);
    graph.nodes = vec![
        Node::new("in", NodeKind::MidiInput),
        Node::new("shift", NodeKind::MidiTranspose { semitones: 12 }),
        Node::new("out", NodeKind::MidiOutput),
    ];
    graph.connections = vec![Connection::new("in", "shift"), Connection::new("shift", "out")];
    let encoded = graph.to_sd().expect("valid MIDI graph serializes");
    assert_eq!(DeviceGraph::from_sd(&encoded).map(|g| g.role), Ok(DeviceRole::MidiEffect));
    let mut prepared = graph.compile_midi().expect("valid MIDI graph");
    let input = [
        (5, NoteEvent::On { key: 60, velocity: 0.75 }),
        (9, NoteEvent::Off { key: 60, velocity: 0.25 }),
        (12, NoteEvent::Controller { number: 64, value: 1.0 }),
        (16, NoteEvent::On { key: 120, velocity: 1.0 }),
    ];
    let mut output = [(0, NoteEvent::AllNotesOff); 4];
    let count = prepared.process(&input, &mut output);
    assert_eq!(count, 3);
    assert_eq!(output[0], (5, NoteEvent::On { key: 72, velocity: 0.75 }));
    assert_eq!(output[1], (9, NoteEvent::Off { key: 72, velocity: 0.25 }));
    assert_eq!(output[2], (12, NoteEvent::Controller { number: 64, value: 1.0 }));
}

#[test]
fn instrument_graph_reads_midi_gate_at_exact_event_frames() {
    use suhina_device_api::NoteEvent;
    let mut graph = DeviceGraph::new("org.test.instrument", "Gated Tone", DeviceRole::Instrument);
    graph.nodes = vec![
        Node::new("gate", NodeKind::MidiGate),
        Node::new("frequency", NodeKind::Constant { value: 440.0 }),
        Node::new("osc", NodeKind::Oscillator { shape: Waveform::Sine }),
        Node::new("amp", NodeKind::Multiply),
        Node::audio_output("out"),
    ];
    graph.connections = vec![
        Connection::ports("frequency", "out", "osc", "frequency"),
        Connection::ports("gate", "out", "amp", "a"),
        Connection::ports("osc", "out", "amp", "b"),
        Connection::new("amp", "out"),
    ];
    let mut instrument = graph.compile_instrument_at(48_000.0).expect("valid instrument graph");
    let events = [(0, NoteEvent::On { key: 60, velocity: 1.0 }), (16, NoteEvent::Off { key: 60, velocity: 0.5 })];
    let mut audio = [0.0; 64];
    instrument.process_instrument_context(&ProcessContext::default(), &events, &mut audio);
    assert!(audio[..32].iter().any(|sample| sample.abs() > 0.01));
    assert!(audio[32..].iter().all(|sample| *sample == 0.0));
}
