// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;
use suhina_test_support::ProjectBuilder;

fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    ProjectBuilder::new().midi_track(1, "Keys").build_without_ids()
}

#[test]
fn lists_the_built_in_devices_and_their_parameters() {
    let mut session = Session::new(project());
    let list = run(&mut session, "device.list", json!({})).unwrap();
    let fm = list.as_array().unwrap().iter().find(|d| d["id"] == "fm-synth").unwrap();
    assert_eq!(fm["role"], "instrument");
    let algorithm = fm["params"].as_array().unwrap().iter().find(|p| p["id"] == "algorithm").unwrap();
    assert_eq!(
        (algorithm["kind"].as_str(), algorithm["min"].as_f64(), algorithm["max"].as_f64()),
        (Some("integer"), Some(1.0), Some(32.0))
    );
    let utility = list.as_array().unwrap().iter().find(|d| d["id"] == "utility").unwrap();
    assert_eq!(utility["role"], "effect");
    let gain = utility["params"].as_array().unwrap().iter().find(|p| p["id"] == "gain").unwrap();
    assert_eq!(gain["unit"], "dB");
}

#[test]
fn devices_are_added_set_switched_and_removed_with_undo() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": "Keys", "id": "utility" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "Keys", "id": "overdrive" })).unwrap();
    // An instrument always goes first.
    let added = run(&mut session, "device.add", json!({ "track": "Keys", "id": "fm-synth" })).unwrap();
    assert_eq!(added["device"], 0);
    let set =
        run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "algorithm", "value": 40 }))
            .unwrap();
    assert_eq!(set["value"], 32.0, "limited to the range");
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "algorithm", "value": 5.4 }))
        .unwrap();
    run(&mut session, "device.setEnabled", json!({ "track": 1, "device": 2, "enabled": false })).unwrap();

    let devices = run(&mut session, "track.devices", json!({ "track": "Keys" })).unwrap();
    let names: Vec<&str> = devices.as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["FM Synth", "Utility", "Overdrive"]);
    let algorithm = devices[0]["params"].as_array().unwrap().iter().find(|p| p["id"] == "algorithm").unwrap();
    assert_eq!(algorithm["value"], 5.0);
    assert_eq!(devices[2]["enabled"], false);
    assert_eq!(session.project().channels[0].devices[0].params, [DeviceParam::new("algorithm", 5.0)]);

    run(&mut session, "device.remove", json!({ "track": 1, "device": 1 })).unwrap();
    assert_eq!(session.project().channels[0].devices.len(), 2);
    for _ in 0..7 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn continuous_parameter_updates_share_one_undo_step() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    for value in [1.0, 2.0, 3.0] {
        run(
            &mut session,
            "device.setParam",
            json!({ "track": 1, "device": 0, "param": "gain", "value": value, "continuous": true }),
        )
        .unwrap();
    }
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "gain", "value": 3.0 })).unwrap();

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels[0].devices[0].params.is_empty());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels[0].devices.is_empty());
}

#[test]
fn saving_during_a_parameter_gesture_keeps_later_updates_dirty() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let path = std::env::temp_dir().join(format!("suhina-param-gesture-{}.suhina", std::process::id()));
    for value in [1.0, 2.0, 3.0] {
        run(
            &mut session,
            "device.setParam",
            json!({ "track": 1, "device": 0, "param": "gain", "value": value, "continuous": true }),
        )
        .unwrap();
        assert!(session.has_unsaved_changes());
        if value < 3.0 {
            run(&mut session, "project.save", json!({ "path": path })).unwrap();
            assert!(!session.has_unsaved_changes());
        }
    }
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "gain", "value": 3.0 })).unwrap();
    assert!(session.has_unsaved_changes());
    let (saved, _) = Session::open(&path).unwrap();
    assert_eq!(saved.project().channels[0].devices[0].params[0].value, 2.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels[0].devices[0].params.is_empty());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].devices[0].params[0].value, 3.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn bad_device_edits_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth" })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("device.add", json!({ "track": 1, "id": "no-such-device" })),
        ("device.add", json!({ "track": 1, "id": "utility", "position": 0 })),
        ("device.add", json!({ "track": 1, "id": "utility", "position": 5 })),
        ("device.add", json!({ "track": 9, "id": "utility" })),
        ("device.remove", json!({ "track": 1, "device": 3 })),
        ("device.setParam", json!({ "track": 1, "device": 0, "param": "nothing", "value": 1 })),
        ("device.setParam", json!({ "track": 1, "device": 0, "param": "feedback", "value": "a lot" })),
        ("device.setParam", json!({ "track": 1, "device": 1, "param": "feedback", "value": 1 })),
        ("device.setEnabled", json!({ "track": 1, "device": 0, "enabled": "yes" })),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
        assert_eq!(session.project(), &before);
    }
    // One undo step for the one good edit.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(run(&mut session, "edit.undo", json!({})).is_err());
}

#[test]
fn sd_effects_load_into_tracks_and_expose_their_parameters() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("org.example.sd-gain", "SDK Gain", suhina_device_sdk::DeviceRole::Effect);
    graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Output", (0.0, 2.0), 1.0));
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("input"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("output"),
    ];
    graph.nodes[1].visible = false;
    graph.connections =
        vec![suhina_device_sdk::Connection::new("input", "gain"), suhina_device_sdk::Connection::new("gain", "output")];
    let path = std::env::temp_dir().join(format!("suhina-test-{}-gain.sd", std::process::id()));
    std::fs::write(&path, graph.to_sd().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    let mut session = Session::new(project());
    let added = run(&mut session, "device.addSd", json!({ "track": 1, "path": path })).unwrap();
    assert_eq!(added["name"], "SDK Gain");
    assert_eq!(session.project().channels[0].devices[0].kind, DeviceKind::Sd);
    let listed = run(&mut session, "track.devices", json!({ "track": 1 })).unwrap();
    assert_eq!(listed[0]["params"][0]["name"], "Output");
    assert_eq!(listed[0]["nodes"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        run(
            &mut session,
            "device.setParam",
            json!({
                "track": 1, "device": 0, "param": "gain", "value": 0.25
            })
        )
        .unwrap()["value"],
        0.25
    );
    assert_eq!(session.project().channels[0].devices[0].params[0].value, 0.25);
    let created = run(&mut session, "track.add", json!({ "sd": path.display().to_string() })).unwrap();
    assert_eq!(created["kind"], "audio");
    assert_eq!(created["device"]["kind"], "sd");
    let _ = std::fs::remove_file(path);
}

#[test]
fn sd_device_updates_migrate_values_atomically_and_undo_to_the_embedded_revision() {
    use suhina_device_sdk::{
        Connection, DeviceGraph, DeviceMigration, DeviceRole, Node, Parameter, ParameterIdMigration,
    };

    let mut old = DeviceGraph::new("org.example.versioned", "Old Gain", DeviceRole::Effect);
    old.parameters.push(Parameter::new("gain", "Gain", (0.0, 2.0), 1.0));
    old.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    old.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];

    let mut newer = DeviceGraph::new("org.example.versioned", "New Gain", DeviceRole::Effect);
    newer.device_version = 2;
    newer.migrations.push(DeviceMigration {
        from_version: 1,
        parameter_ids: vec![ParameterIdMigration { from: "gain".into(), to: "level".into() }],
    });
    newer.parameters.push(Parameter::new("level", "Level", (0.0, 4.0), 0.5));
    newer.nodes = vec![Node::audio_input("in"), Node::gain("gain", "level"), Node::audio_output("out")];
    newer.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];

    let old_path = std::env::temp_dir().join(format!("suhina-test-{}-old.sd", std::process::id()));
    let new_path = std::env::temp_dir().join(format!("suhina-test-{}-new.sd", std::process::id()));
    std::fs::write(&old_path, old.to_sd().expect("old device serializes")).expect("write old device");
    std::fs::write(&new_path, newer.to_sd().expect("new device serializes")).expect("write new device");

    let mut session = Session::new(project());
    run(&mut session, "device.addSd", json!({"track":1,"path":old_path})).unwrap();
    run(&mut session, "device.setParam", json!({"track":1,"device":0,"param":"gain","value":1.75})).unwrap();
    let result = run(&mut session, "device.updateSd", json!({"track":1,"device":0,"path":new_path})).unwrap();
    assert_eq!(result["from_version"], 1);
    assert_eq!(result["to_version"], 2);
    let device = &session.project().channels[0].devices[0];
    assert_eq!(device.name, "New Gain");
    assert_eq!(device.params, vec![DeviceParam::new("level", 1.75)]);
    let state = String::from_utf8(device.state.clone().expect("updated graph is embedded")).unwrap();
    assert_eq!(DeviceGraph::from_sd(&state).unwrap().device_version, 2);

    run(&mut session, "edit.undo", json!({})).unwrap();
    let device = &session.project().channels[0].devices[0];
    assert_eq!(device.name, "Old Gain");
    assert_eq!(device.params, vec![DeviceParam::new("gain", 1.75)]);
    let _ = std::fs::remove_file(old_path);
    let _ = std::fs::remove_file(new_path);
}

#[test]
fn sd_midi_effects_and_instruments_insert_on_midi_tracks_in_chain_order() {
    let mut midi_effect =
        suhina_device_sdk::DeviceGraph::new("test.transpose", "Transpose", suhina_device_sdk::DeviceRole::MidiEffect);
    midi_effect.nodes = vec![
        suhina_device_sdk::Node::new("in", suhina_device_sdk::NodeKind::MidiInput),
        suhina_device_sdk::Node::new("transpose", suhina_device_sdk::NodeKind::MidiTranspose { semitones: 12 }),
        suhina_device_sdk::Node::new("out", suhina_device_sdk::NodeKind::MidiOutput),
    ];
    midi_effect.connections = vec![
        suhina_device_sdk::Connection::new("in", "transpose"),
        suhina_device_sdk::Connection::new("transpose", "out"),
    ];
    let midi_path = std::env::temp_dir().join(format!("suhina-test-{}-midi.sd", std::process::id()));
    std::fs::write(&midi_path, midi_effect.to_sd().expect("valid MIDI effect")).expect("write test file");
    let mut session = Session::new(project());
    assert_eq!(run(&mut session, "device.addSd", json!({ "track": 1, "path": midi_path })).unwrap()["device"], 0);
    assert_eq!(run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth" })).unwrap()["device"], 1);
    assert_eq!(run(&mut session, "track.devices", json!({ "track": 1 })).unwrap()[0]["role"], "midi_effect");
    assert_eq!(
        session.project().channels[0].devices[0]
            .state
            .as_ref()
            .map(|s| String::from_utf8_lossy(s).contains("midi_effect")),
        Some(true)
    );

    let mut instrument =
        suhina_device_sdk::DeviceGraph::new("test.instrument", "Instrument", suhina_device_sdk::DeviceRole::Instrument);
    instrument.nodes = vec![
        suhina_device_sdk::Node::new("gate", suhina_device_sdk::NodeKind::MidiGate),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    instrument.connections = vec![suhina_device_sdk::Connection::new("gate", "out")];
    let instrument_path = std::env::temp_dir().join(format!("suhina-test-{}-instrument.sd", std::process::id()));
    std::fs::write(&instrument_path, instrument.to_sd().expect("valid instrument")).expect("write test file");
    let mut blank = project();
    blank.channels[0].devices.clear();
    let mut session = Session::new(blank);
    assert_eq!(run(&mut session, "device.addSd", json!({ "track": 1, "path": instrument_path })).unwrap()["device"], 0);
    assert_eq!(run(&mut session, "track.devices", json!({ "track": 1 })).unwrap()[0]["role"], "instrument");
    assert_eq!(
        session.project().channels[0].devices[0]
            .state
            .as_ref()
            .map(|s| String::from_utf8_lossy(s).contains("instrument")),
        Some(true)
    );
    let _ = std::fs::remove_file(midi_path);
    let _ = std::fs::remove_file(instrument_path);
}

/// The pitch effect is listed as a MIDI effect, goes before the instrument and the audio
/// effects, keeps its values in range and goes only on MIDI tracks.
#[test]
fn the_pitch_midi_effect_goes_before_the_instrument_on_midi_tracks() {
    let mut session = Session::new(project());
    let list = run(&mut session, "device.list", json!({})).unwrap();
    let pitch = list.as_array().unwrap().iter().find(|d| d["id"] == "pitch").unwrap();
    assert_eq!(pitch["role"], "midi_effect");
    let unit = |id: &str| pitch["params"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()["unit"].clone();
    assert_eq!(
        (unit("pitch"), unit("pitchDegrees"), unit("lowest")),
        (json!("semitones"), json!("scale degrees"), json!("key"))
    );

    assert_eq!(run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap()["device"], 0);
    // A MIDI effect goes before the audio effects, the instrument after the MIDI effects.
    assert_eq!(run(&mut session, "device.add", json!({ "track": 1, "id": "pitch" })).unwrap()["device"], 0);
    assert_eq!(run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth" })).unwrap()["device"], 1);
    assert_eq!(run(&mut session, "device.add", json!({ "track": 1, "id": "pitch" })).unwrap()["device"], 1);
    let devices = run(&mut session, "track.devices", json!({ "track": 1 })).unwrap();
    let order: Vec<(&str, &str)> =
        devices.as_array().unwrap().iter().map(|d| (d["id"].as_str().unwrap(), d["role"].as_str().unwrap())).collect();
    assert_eq!(
        order,
        [("pitch", "midi_effect"), ("pitch", "midi_effect"), ("fm-synth", "instrument"), ("utility", "effect")]
    );
    let error = run(&mut session, "device.add", json!({ "track": 1, "id": "pitch", "position": 3 })).unwrap_err();
    assert!(error.contains("before the instrument"), "{error}");
    let set = run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "pitch", "value": 200 }))
        .unwrap();
    assert_eq!(set["value"], 128.0, "limited to the range");

    // Not on an audio track; a new track for it is a MIDI track.
    run(&mut session, "track.add", json!({ "kind": "audio", "name": "Drums" })).unwrap();
    let error = run(&mut session, "device.add", json!({ "track": "Drums", "id": "pitch" })).unwrap_err();
    assert!(error.contains("MIDI track"), "{error}");
    let error = run(&mut session, "track.add", json!({ "kind": "audio", "device": "pitch" })).unwrap_err();
    assert!(error.contains("MIDI track"), "{error}");
    let added = run(&mut session, "track.add", json!({ "device": "pitch" })).unwrap();
    let track = session.project().tracks.iter().find(|t| t.name == "Pitch").unwrap();
    assert!(track.content.notes, "{added}");
}

/// No audio effect, from any of the three add commands, and no `.sd` instrument goes before the
/// MIDI effects, with or without an instrument, and no audio effect before the instrument.
#[test]
fn nothing_goes_before_the_midi_effects_but_a_midi_effect() {
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node};
    let mut effect = DeviceGraph::new("test.pass", "Pass", DeviceRole::Effect);
    effect.nodes = vec![Node::audio_input("in"), Node::audio_output("out")];
    effect.connections = vec![Connection::new("in", "out")];
    let effect_path = std::env::temp_dir().join(format!("suhina-test-{}-order-effect.sd", std::process::id()));
    std::fs::write(&effect_path, effect.to_sd().expect("valid effect")).expect("write test file");
    let mut instrument = DeviceGraph::new("test.order-instrument", "Instrument", DeviceRole::Instrument);
    instrument.nodes = vec![Node::new("gate", suhina_device_sdk::NodeKind::MidiGate), Node::audio_output("out")];
    instrument.connections = vec![Connection::new("gate", "out")];
    let instrument_path = std::env::temp_dir().join(format!("suhina-test-{}-order-instrument.sd", std::process::id()));
    std::fs::write(&instrument_path, instrument.to_sd().expect("valid instrument")).expect("write test file");

    let mut session = Session::new(project());
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "pitch" })).unwrap();
    let before = session.project().clone();
    // Without an instrument, an effect still goes after the MIDI effects.
    for (id, params) in [
        ("device.add", json!({ "track": 1, "id": "utility", "position": 0 })),
        ("device.addSd", json!({ "track": 1, "path": effect_path, "position": 0 })),
        ("device.addPlugin", json!({ "track": 1, "plugin": "Verb", "position": 0 })),
        // An instrument goes right after them, not before.
        ("device.addSd", json!({ "track": 1, "path": instrument_path, "position": 0 })),
    ] {
        let error = run(&mut session, id, params.clone()).unwrap_err();
        assert!(error.contains("after the track's"), "{id} {params}: {error}");
        assert_eq!(session.project(), &before);
    }
    assert_eq!(
        run(&mut session, "device.add", json!({ "track": 1, "id": "utility", "position": 1 })).unwrap()["device"],
        1
    );
    assert_eq!(
        run(&mut session, "device.addSd", json!({ "track": 1, "path": instrument_path, "position": 1 })).unwrap()["device"],
        1
    );
    // With the instrument there, effects go after it too.
    for (id, params) in [
        ("device.add", json!({ "track": 1, "id": "utility", "position": 1 })),
        ("device.addSd", json!({ "track": 1, "path": effect_path, "position": 1 })),
        ("device.addPlugin", json!({ "track": 1, "plugin": "Verb", "position": 1 })),
    ] {
        let error = run(&mut session, id, params.clone()).unwrap_err();
        assert!(error.contains("position 2 to 3"), "{id} {params}: {error}");
    }
    assert_eq!(
        run(&mut session, "device.addSd", json!({ "track": 1, "path": effect_path, "position": 2 })).unwrap()["device"],
        2
    );
    assert_eq!(
        run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Verb", "position": 2 })).unwrap()["device"],
        2
    );
    let _ = std::fs::remove_file(effect_path);
    let _ = std::fs::remove_file(instrument_path);
}

/// A built-in or `.sd` MIDI effect goes among the MIDI effects at the start of the chain, never
/// after an audio effect, also on a track with no instrument.
#[test]
fn a_midi_effect_never_goes_after_an_audio_effect() {
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind};
    let mut transpose = DeviceGraph::new("test.order-transpose", "Transpose", DeviceRole::MidiEffect);
    transpose.nodes = vec![
        Node::new("in", NodeKind::MidiInput),
        Node::new("transpose", NodeKind::MidiTranspose { semitones: 12 }),
        Node::new("out", NodeKind::MidiOutput),
    ];
    transpose.connections = vec![Connection::new("in", "transpose"), Connection::new("transpose", "out")];
    let path = std::env::temp_dir().join(format!("suhina-test-{}-order-transpose.sd", std::process::id()));
    std::fs::write(&path, transpose.to_sd().expect("valid MIDI effect")).expect("write test file");

    // Pitch then Utility, and no instrument.
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "pitch" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("device.addSd", json!({ "track": 1, "path": path, "position": 2 })),
        ("device.add", json!({ "track": 1, "id": "pitch", "position": 2 })),
    ] {
        let error = run(&mut session, id, params.clone()).unwrap_err();
        assert!(error.contains("position 0 to 1"), "{id} {params}: {error}");
        assert_eq!(session.project(), &before);
    }
    // Without a position it goes after the MIDI effects, before the audio effect.
    assert_eq!(run(&mut session, "device.addSd", json!({ "track": 1, "path": path })).unwrap()["device"], 1);
    assert_eq!(
        run(&mut session, "device.addSd", json!({ "track": 1, "path": path, "position": 0 })).unwrap()["device"],
        0
    );
    let roles: Vec<Value> = run(&mut session, "track.devices", json!({ "track": 1 }))
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["role"].clone())
        .collect();
    assert_eq!(roles, [json!("midi_effect"), json!("midi_effect"), json!("midi_effect"), json!("effect")]);
    let _ = std::fs::remove_file(path);
}

/// A catalog with one installed effect and one instrument, as a scan would have found them.
fn plugin_env() -> std::sync::Arc<suhina_session::PluginEnv> {
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let mut catalog = Catalog::new();
    let plugin = |file: &str, id: &str, name: &str, features: &[&str]| PluginInfo {
        path: std::path::PathBuf::from(file),
        id: id.into(),
        name: name.into(),
        vendor: "Vendor".into(),
        version: "1.0".into(),
        features: features.iter().map(|f| f.to_string()).collect(),
        replaces: Vec::new(),
        ara: false,
    };
    let verb = plugin("Verb.vst3", "ABCDEF0123456789ABCDEF0123456789", "Verb", &["Fx|Reverb"]);
    let synth = plugin("Synth.clap", "com.vendor.synth", "Synth", &["instrument", "synthesizer"]);
    catalog.insert(verb.path.clone(), vec![verb]);
    catalog.insert(synth.path.clone(), vec![synth]);
    let tune = PluginInfo {
        ara: true,
        ..plugin("Tune.vst3", "00112233445566778899AABBCCDDEEFF", "Tune", &["Fx|Pitch Shift"])
    };
    catalog.insert(tune.path.clone(), vec![tune]);
    std::sync::Arc::new(suhina_session::PluginEnv::new(catalog, "no-such-host".into()))
}

#[test]
fn installed_plug_ins_are_added_from_the_catalog() {
    let mut session = Session::new(project());
    // Without a catalog there is nothing to add from.
    let error = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Verb" })).unwrap_err();
    assert!(error.contains("catalog"), "{error}");
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let verb = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "verb" })).unwrap();
    assert_eq!(
        (verb["device"].clone(), verb["format"].clone(), verb["instrument"].clone()),
        (json!(1), json!("vst3"), json!(false))
    );
    // An instrument goes first, and only first.
    let synth = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "com.vendor.synth" })).unwrap();
    assert_eq!(synth["device"], 0);
    assert!(run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Synth", "position": 2 })).is_err());
    assert!(run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Nothing" })).is_err());
    let devices = &session.project().channels[0].devices;
    assert_eq!(devices[0].kind, DeviceKind::Clap { id: "com.vendor.synth".into() });
    assert_eq!(devices[2].kind, DeviceKind::Vst3 { id: "ABCDEF0123456789ABCDEF0123456789".into() });
    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].devices.len(), 2);
}

#[test]
fn a_clip_opens_in_the_installed_plug_in_that_edits_clips() {
    let mut session = Session::new(project());
    assert_eq!(session.ara_plugin(), None, "no catalog");
    session.set_plugin_env(plugin_env());
    let (name, spec) = session.ara_plugin().unwrap();
    assert_eq!(name, "Tune");
    assert!(spec.starts_with("vst3:"), "{spec}");
    // A scan that missed it: Melodyne is still chosen by its name, and the host finds it
    // supports ARA when it loads.
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let melodyne = PluginInfo {
        path: "Melodyne.vst3".into(),
        id: "5653544D656C6F6D656C6F64796E6535".into(),
        name: "Melodyne".into(),
        vendor: "Celemony".into(),
        version: "5.4.2".into(),
        features: vec!["Fx".into()],
        replaces: Vec::new(),
        ara: false,
    };
    let mut catalog = Catalog::new();
    catalog.insert(melodyne.path.clone(), vec![melodyne]);
    session.set_plugin_env(std::sync::Arc::new(suhina_session::PluginEnv::new(catalog, "no-such-host".into())));
    assert_eq!(session.ara_plugin().map(|(name, _)| name).as_deref(), Some("Melodyne"));
}

#[test]
fn a_plug_in_that_edits_clips_goes_first() {
    let mut session = Session::new(project());
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    // Asked for after the utility, it goes before it, as it plays the clips from their files.
    let tune = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Tune", "position": 1 })).unwrap();
    assert_eq!(
        (tune["device"].clone(), tune["ara"].clone(), tune["moved_first"].clone()),
        (json!(0), json!(true), json!(true))
    );
    let devices = &session.project().channels[0].devices;
    assert_eq!(devices[0].name, "Tune");
    // Without a position it goes first too, and says nothing moved.
    run(&mut session, "edit.undo", json!({})).unwrap();
    let tune = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Tune" })).unwrap();
    assert_eq!((tune["device"].clone(), tune["moved_first"].clone()), (json!(0), json!(false)));
}

#[test]
fn plug_in_commands_need_a_plug_in_and_a_playing_project() {
    let mut session = Session::new(project());
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Verb" })).unwrap();
    let device = json!({ "track": 1, "device": 1 });
    // Not playing: its plug-in does not run, and there is no editor to open.
    assert_eq!(run(&mut session, "device.plugin", device.clone()).unwrap()["status"], "stopped");
    let listed = run(&mut session, "track.devices", json!({ "track": 1 })).unwrap();
    assert_eq!(listed[1]["plugin"]["status"], "stopped");
    assert!(listed[0].get("plugin").is_none(), "a built-in effect or instrument has no plug-in");
    assert!(run(&mut session, "device.openEditor", device.clone()).is_err());
    assert!(run(&mut session, "device.restart", device).is_err());
    let built_in = run(&mut session, "device.openEditor", json!({ "track": 1, "device": 0 })).unwrap_err();
    assert!(built_in.contains("not a plug-in"), "{built_in}");
    let kinds: Vec<(&str, &str)> = suhina_session::commands()
        .iter()
        .filter(|c| c.id.starts_with("device.") && c.id.contains("Editor") || c.id == "device.restart")
        .map(|c| (c.id, c.kind.name()))
        .collect();
    assert_eq!(
        kinds,
        [("device.openEditor", "plugin"), ("device.closeEditor", "plugin"), ("device.restart", "plugin")]
    );
}

#[test]
fn sdk_catalog_is_a_read_command_and_matches_the_sdk() {
    let mut session = Session::new(project());
    let catalog = run(&mut session, "device.sdNodes", json!({})).unwrap();
    assert_eq!(catalog["version"], suhina_device_sdk::VERSION);
    assert_eq!(catalog["nodes"].as_array().unwrap().len(), suhina_device_sdk::NODE_CATALOG.len());
    assert!(
        catalog["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == "delay" && n["inputs"][1]["name"] == "time")
    );
    assert!(run(&mut session, "edit.undo", json!({})).is_err());
}

#[test]
fn branching_sd_graph_and_discrete_parameters_survive_project_save_and_source_removal() {
    use suhina_device_sdk::{
        Connection as C, DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter, ParameterControl,
    };
    let mut g = DeviceGraph::new("org.test.stereo", "Stereo", DeviceRole::Effect);
    let mut p = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    p.control = ParameterControl::Integer;
    p.unit = "steps".into();
    g.parameters.push(p);
    g.nodes = vec![N::audio_input("in"), N::new("stereo", K::Stereo), N::gain("gain", "gain"), N::audio_output("out")];
    g.connections = vec![
        C::ports("in", "left", "stereo", "right"),
        C::ports("in", "right", "stereo", "left"),
        C::new("stereo", "gain"),
        C::new("gain", "out"),
    ];
    g.editor = Some(suhina_device_sdk::editor::Editor {
        graph: suhina_device_sdk::editor::GraphLayout {
            nodes: vec![],
            wires: vec![suhina_device_sdk::editor::WireLayout {
                connection: C::ports("in", "left", "stereo", "right"),
                points: vec![[50.0, 60.0]],
                from_anchor: None,
                to_anchor: None,
            }],
        },
    });
    let path = std::env::temp_dir().join(format!("suhina-sdk-branching-{}.sd", std::process::id()));
    g.write_file(&path).unwrap();
    let mut session = Session::new(project());
    run(&mut session, "device.addSd", json!({"track":1,"path":path})).unwrap();
    std::fs::remove_file(&path).unwrap();
    let listed = run(&mut session, "track.devices", json!({"track":1})).unwrap();
    assert_eq!(listed[0]["params"][0]["control"], "integer");
    assert_eq!(listed[0]["params"][0]["unit"], "steps");
    let changed =
        run(&mut session, "device.setParam", json!({"track":1,"device":0,"param":"gain","value":1.6})).unwrap();
    assert_eq!(changed["value"], 2.0);
    let restored = suhina_project_file::from_json(&suhina_project_file::to_json(session.project())).unwrap();
    let device = &restored.channels[0].devices[0];
    let graph = DeviceGraph::from_sd(std::str::from_utf8(device.state.as_deref().unwrap()).unwrap()).unwrap();
    assert_eq!(graph, g);
    assert_eq!(device.params[0].value, 2.0);
    let mut effect = graph.compile().unwrap();
    effect.set_parameter(0, device.params[0].value);
    let mut out = [1.0, 2.0];
    effect.process(&mut out);
    assert_eq!(out, [4.0, 2.0]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].devices[0].params[0].value, 1.0);
    run(&mut session, "edit.redo", json!({})).unwrap();
    let state = session.project().channels[0].devices[0].state.as_deref().unwrap();
    assert_eq!(DeviceGraph::from_sd(std::str::from_utf8(state).unwrap()).unwrap(), g);
}

#[test]
fn xy_gesture_groups_axes_and_ends_on_unchanged_value() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"track":1,"id":"utility"})).unwrap();
    let before = session.project().clone();
    for (param, value, continuous) in [("gain", 2.0, true), ("balance", 0.5, true), ("balance", 0.5, false)] {
        run(
            &mut session,
            "device.setParam",
            json!({"track":1,"device":0,"param":param,"value":value,"continuous":continuous,"gesture":"xy"}),
        )
        .unwrap();
    }
    let after = session.project().clone();
    run(
        &mut session,
        "device.setParam",
        json!({"track":1,"device":0,"param":"gain","value":3.0,"continuous":false,"gesture":"xy"}),
    )
    .unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &after);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before);
    assert!(
        run(&mut session, "device.setParam", json!({"track":1,"device":0,"param":"gain","value":3.0,"gesture":""}))
            .is_err()
    );
}

#[path = "../../device-sdk/examples/delay.rs"]
mod delay_panel_example;

#[test]
fn sdk_linked_delay_values_update_both_directions_and_undo_together() {
    let graph = delay_panel_example::build().unwrap();
    let path = std::env::temp_dir().join(format!("suhina-linked-delay-{}.sd", std::process::id()));
    graph.write_file(&path).unwrap();
    let mut session = Session::new(project());
    run(&mut session, "device.addSd", json!({"track":1,"path":path})).unwrap();
    std::fs::remove_file(path).unwrap();
    fn value(session: &Session, id: &str) -> f64 {
        session.project().channels[0].devices[0].params.iter().find(|p| p.id == id).unwrap().value
    }
    fn edit(session: &mut Session, id: &str, value: f64) {
        run(session, "device.setParam", json!({"track":1,"device":0,"param":id,"value":value})).unwrap();
    }
    edit(&mut session, "time_right", 427.);
    assert_eq!(value(&session, "time_left"), 427.);
    assert_eq!(value(&session, "time_right"), 427.);
    edit(&mut session, "sync_right", 0.);
    assert_eq!(value(&session, "sync_left"), 0.);
    let saved = suhina_project_file::to_json(session.project());
    assert_eq!(suhina_project_file::from_json(&saved).unwrap(), *session.project());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(value(&session, "sync_left"), 1.);
    assert_eq!(value(&session, "sync_right"), 1.);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(value(&session, "sync_left"), 0.);
    edit(&mut session, "link", 0.);
    edit(&mut session, "time_right", 640.);
    assert_eq!(value(&session, "time_left"), 427.);
    edit(&mut session, "link", 1.);
    assert_eq!(value(&session, "time_right"), 427.);
    for (v, continuous) in [(500., true), (520., true), (550., false)] {
        run(&mut session,"device.setParam",json!({"track":1,"device":0,"param":"time_right","value":v,"continuous":continuous,"gesture":"linked-time"})).unwrap();
    }
    assert_eq!(value(&session, "time_left"), 550.);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(value(&session, "time_left"), 427.);
    assert_eq!(value(&session, "time_right"), 427.);
    edit(&mut session, "division_left", 16.);
    assert_eq!(value(&session, "division_right"), 16.);
    let saved_path = std::env::temp_dir().join(format!("suhina-sdk-delay-panel-{}.suhina", std::process::id()));
    run(&mut session, "project.save", json!({ "path": saved_path })).unwrap();
    let (reopened, _) = Session::open(&saved_path).unwrap();
    assert_eq!(reopened.project(), session.project(), "panel and linked values survive save/reopen");
    std::fs::remove_file(saved_path).unwrap();
}

/// The NAM devices take a file each: the model's or IR's path in `Device::state`, saved with the
/// project ([docs/spec/nam-amp-and-cab.md] › What the project stores).
#[test]
fn a_nam_device_loads_its_file_with_the_command() {
    let dir = std::env::temp_dir().join(format!("suhina-devices-nam-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder is made");
    let models = dir.join("NAM models");
    std::fs::create_dir_all(&models).expect("the models folder is made");
    let irs = dir.join("IRs");
    std::fs::create_dir_all(&irs).expect("the IR folder is made");
    // A tiniest WaveNet model, the same fixture the device's tests run on.
    let model_json = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": {
            "layers": [{
                "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                "gated": false, "head_bias": false
            }],
            "head": null, "head_scale": 1.0
        },
        "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
        "metadata": { "name": "Test amp 1965" }
    }"#;
    let model_path = models.join("test-amp.nam");
    std::fs::write(&model_path, model_json).unwrap();
    // A two-tap IR mono wav.
    let ir_path = irs.join("cabinet.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&ir_path, spec).unwrap();
    writer.write_sample(16_384_i16).unwrap();
    drop(writer);

    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-cab-loader" })).unwrap();

    let amp_path = model_path.display().to_string();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": &amp_path })).unwrap();
    // The state holds the absolute path; the parsed model is in the session's assets.
    let file_info = run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(file_info["file"], amp_path);
    assert_eq!(file_info["name"], "Test amp 1965");
    let asset = session.device_asset(&amp_path).unwrap();
    assert!(matches!(asset, suhina_device_registry::DeviceAsset::Nam(_)));

    let ir_file = ir_path.display().to_string();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 1, "path": &ir_file })).unwrap();
    let ir = run(&mut session, "device.file", json!({ "track": 1, "device": 1 })).unwrap();
    assert_eq!(ir["file"], ir_file);
    // The cab's shown name is its parent folder's, per the spec (its files are the cabs).
    assert_eq!(ir["name"], "IRs");
    assert!(matches!(session.device_asset(&ir_file), Some(suhina_device_registry::DeviceAsset::Ir(_))));

    // A wrong extension is refused with a message that names what it wants.
    let error =
        run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": &ir_file })).unwrap_err();
    assert!(error.contains(".nam"), "{error}");

    // A behaviour after a failed read: a file that is not a model leaves a note.
    let broken = models.join("broken.nam");
    std::fs::write(&broken, "{not json").unwrap();
    let error = run(
        &mut session,
        "device.setNamFile",
        json!({ "track": 1, "device": 0, "path": broken.display().to_string() }),
    )
    .unwrap_err();
    assert!(error.contains("not a NAM model"), "{error}");

    // One command, one undo step: two undos take both files back to none.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap()["file"], "");
    assert_eq!(run(&mut session, "device.file", json!({ "track": 1, "device": 1 })).unwrap()["file"], "");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A project's stored path gets the extension check the command makes: opening a project whose
/// NAM device points at, say, another kind of file leaves a note and no asset.
#[test]
fn a_projects_stored_path_gets_the_extension_check() {
    let dir = std::env::temp_dir().join(format!("suhina-devices-nam-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder is made");
    let impostor = dir.join("not-a-model.wav");
    std::fs::write(&impostor, b"RIFF....").unwrap();

    let mut project = project();
    project.channels[0].devices.push(Device {
        state: Some(impostor.display().to_string().into_bytes()),
        ..Device::new("Amp", DeviceKind::Builtin("nam-amp-modeler".into()))
    });
    let file = dir.join("with-nam.suhina");
    suhina_project_file::write_file(&file, &project).expect("the project is written");
    let (session, _) = Session::open(&file).expect("the project opens");
    assert!(session.device_asset(&impostor.display().to_string()).is_none(), "no asset was read");
    let note = session.device_asset_note(&impostor.display().to_string());
    assert!(note.is_some_and(|n| n.contains(".nam")), "{note:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn devices_are_copied_pasted_and_removed_together_in_one_step_each() {
    let mut session = Session::new(project());
    for id in ["utility", "overdrive", "utility"] {
        run(&mut session, "device.add", json!({ "track": "Keys", "id": id })).unwrap();
    }
    let names = |s: &Session| {
        let p = s.project();
        let channel = p.tracks.iter().find(|t| t.name == "Keys").and_then(|t| t.channel).unwrap();
        p.channel(channel).unwrap().devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    };
    let before = names(&session);
    assert_eq!(before.len(), 3);
    // The overdrive's drive changed, then copied with the first device, and pasted after it.
    run(&mut session, "device.copy", json!({ "track": "Keys", "devices": [1, 0] })).unwrap();
    assert_eq!(session.copied_devices(), 2);
    let pasted = run(&mut session, "device.paste", json!({ "track": "Keys", "position": 1 })).unwrap();
    assert_eq!(pasted["devices"], json!([1, 2]));
    let after = names(&session);
    assert_eq!(
        after,
        [before[0].clone(), before[0].clone(), before[1].clone(), before[1].clone(), before[2].clone()],
        "in the chain's order"
    );
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session), before, "one undo step");
    run(&mut session, "device.removeMany", json!({ "track": "Keys", "devices": [0, 2] })).unwrap();
    assert_eq!(names(&session), [before[1].clone()]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session), before, "one undo step");
    assert!(run(&mut session, "device.removeMany", json!({ "track": "Keys", "devices": [7] })).is_err());
}

#[test]
fn picked_devices_are_duplicated_after_the_last_in_one_step_and_the_clipboard_stays() {
    let mut session = Session::new(project());
    for id in ["utility", "overdrive", "delay"] {
        run(&mut session, "device.add", json!({ "track": "Keys", "id": id })).unwrap();
    }
    let names = |s: &Session| {
        let p = s.project();
        let channel = p.tracks.iter().find(|t| t.name == "Keys").and_then(|t| t.channel).unwrap();
        p.channel(channel).unwrap().devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    };
    let before = names(&session);
    run(&mut session, "device.copy", json!({ "track": "Keys", "devices": [2] })).unwrap();
    let made = run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [1, 0] })).unwrap();
    assert_eq!(made["devices"], json!([2, 3]));
    let after = names(&session);
    assert_eq!(after, [&before[0], &before[1], &before[0], &before[1], &before[2]].map(String::clone));
    assert_eq!(session.copied_devices(), 1, "the clipboard is as it was");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session), before, "one undo step");
}

/// A track playing notes with a MIDI effect, an instrument and an effect, in the chain's order.
fn keys_with_the_whole_chain() -> Session {
    let mut session = Session::new(project());
    for id in ["utility", "fm-synth", "pitch"] {
        let added = run(&mut session, "device.add", json!({ "track": "Keys", "id": id }));
        assert!(added.is_ok(), "{added:?}");
    }
    session
}

fn chain_names(session: &Session) -> Vec<String> {
    session.project().channels[0].devices.iter().map(|d| d.name.clone()).collect()
}

#[test]
fn duplicating_devices_keeps_the_chains_rules() {
    let mut session = keys_with_the_whole_chain();
    let before = session.project().clone();
    // The MIDI effect with the instrument: a second instrument, and a MIDI effect after one.
    let error = run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [0, 1] })).unwrap_err();
    assert!(error.contains("MIDI effects go first"), "{error}");
    assert_eq!(session.project(), &before, "a refused duplicate changes nothing");
    // The instrument alone.
    assert!(run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [1] })).is_err());
    assert_eq!(session.project(), &before);
    // The MIDI effect and the effect: the copy of the MIDI effect would come after the effect.
    assert!(run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [0, 2] })).is_err());
    assert_eq!(session.project(), &before);
    // What keeps the rules still works: a MIDI effect that leads the chain, an effect at the end.
    run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [0] })).unwrap();
    assert_eq!(chain_names(&session).len(), 4);
    run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [3] })).unwrap();
    assert_eq!(chain_names(&session).len(), 5);
}

#[test]
fn pasting_devices_keeps_the_chains_rules() {
    let mut session = keys_with_the_whole_chain();
    let before = session.project().clone();
    let copy = |session: &mut Session, device: usize| {
        run(session, "device.copy", json!({ "track": "Keys", "devices": [device] })).unwrap();
    };
    // A second instrument.
    copy(&mut session, 1);
    assert!(run(&mut session, "device.paste", json!({ "track": "Keys" })).is_err());
    // A MIDI effect after the instrument.
    copy(&mut session, 0);
    assert!(run(&mut session, "device.paste", json!({ "track": "Keys" })).is_err());
    assert_eq!(session.project(), &before, "a refused paste changes nothing");
    // A MIDI effect where the MIDI effects are, and an effect at the end, are fine.
    run(&mut session, "device.paste", json!({ "track": "Keys", "position": 0 })).unwrap();
    copy(&mut session, 3);
    run(&mut session, "device.paste", json!({ "track": "Keys" })).unwrap();
    assert_eq!(chain_names(&session).len(), 5);
    // An instrument and a MIDI effect go on a track that plays notes only.
    run(&mut session, "track.add", json!({ "kind": "audio", "name": "Bus" })).unwrap();
    for device in [0, 2] {
        copy(&mut session, device);
        assert!(run(&mut session, "device.paste", json!({ "track": "Bus" })).is_err(), "{device}");
    }
    // A MIDI effect does not go into a group.
    run(&mut session, "device.group", json!({ "track": "Keys", "device": 4 })).unwrap();
    copy(&mut session, 0);
    assert!(run(&mut session, "device.paste", json!({ "track": "Keys", "position": "4/0/0" })).is_err());
}

/// Fills the Keys track's chain with effects up to the most devices a track holds.
fn keys_with_a_full_chain() -> Session {
    let mut session = keys_with_the_whole_chain();
    while chain_names(&session).len() < suhina_model::MAX_DEVICES {
        let added = run(&mut session, "device.add", json!({ "track": "Keys", "id": "utility" }));
        assert!(added.is_ok(), "{added:?}");
    }
    session
}

#[test]
fn duplicating_and_pasting_devices_stay_within_the_device_limit() {
    let mut session = keys_with_a_full_chain();
    let before = session.project().clone();
    let error = run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": [3] })).unwrap_err();
    assert!(error.contains("at most"), "{error}");
    assert_eq!(session.project(), &before, "a refused duplicate changes nothing");
    run(&mut session, "device.copy", json!({ "track": "Keys", "devices": [3] })).unwrap();
    let error = run(&mut session, "device.paste", json!({ "track": "Keys" })).unwrap_err();
    assert!(error.contains("at most"), "{error}");
    assert_eq!(session.project(), &before, "a refused paste changes nothing");
    // One place free: the same copy goes in.
    run(&mut session, "device.remove", json!({ "track": "Keys", "device": 3 })).unwrap();
    run(&mut session, "device.paste", json!({ "track": "Keys" })).unwrap();
    assert_eq!(chain_names(&session).len(), suhina_model::MAX_DEVICES);
}

#[test]
fn pasting_devices_stays_within_the_group_depth_limit() {
    let mut session = keys_with_the_whole_chain();
    // Group the utility four times over: a group nested to the deepest level.
    for _ in 0..suhina_model::MAX_DEVICE_GROUP_DEPTH {
        run(&mut session, "device.group", json!({ "track": "Keys", "device": 2 })).unwrap();
    }
    let before = session.project().clone();
    // A copy of the outer group, put in its innermost branch, would nest one level deeper.
    run(&mut session, "device.copy", json!({ "track": "Keys", "devices": [2] })).unwrap();
    let error = run(&mut session, "device.paste", json!({ "track": "Keys", "position": "2/0/0/0/0/0/0/0/0" }));
    assert!(error.as_ref().is_err_and(|e| e.contains("nest")), "{error:?}");
    assert_eq!(session.project(), &before, "a refused paste changes nothing");
    // Duplicating copies a device beside itself, which never nests deeper than it already is.
    run(&mut session, "device.duplicateMany", json!({ "track": "Keys", "devices": ["2/0/0/0/0/0/0/0/0"] })).unwrap();
}

#[test]
fn a_device_moves_or_copies_to_another_track_in_one_step() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": "Keys", "id": "delay" })).unwrap();
    let chain = |s: &Session, name: &str| {
        let p = s.project();
        let channel = p.tracks.iter().find(|t| t.name == name).and_then(|t| t.channel).unwrap();
        p.channel(channel).unwrap().devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    };
    run(&mut session, "track.add", json!({ "kind": "audio", "name": "Bus" })).unwrap();
    let other = "Bus".to_string();
    let (keys, there) = (chain(&session, "Keys"), chain(&session, &other));
    let at = keys.len() - 1;
    run(&mut session, "device.moveToTrack", json!({ "track": "Keys", "device": at, "toTrack": other, "copy": true }))
        .unwrap();
    assert_eq!(chain(&session, "Keys"), keys, "a copy leaves it");
    assert_eq!(chain(&session, &other).len(), there.len() + 1);
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "device.moveToTrack", json!({ "track": "Keys", "device": at, "toTrack": other })).unwrap();
    assert_eq!(chain(&session, "Keys").len(), keys.len() - 1, "moved away");
    assert_eq!(chain(&session, &other).last().map(String::as_str), keys.last().map(String::as_str));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!((chain(&session, "Keys"), chain(&session, &other)), (keys, there), "one step");
    assert!(
        run(&mut session, "device.moveToTrack", json!({ "track": "Keys", "device": 0, "toTrack": "Keys" })).is_err()
    );
}

#[test]
fn an_instrument_group_moved_to_another_track_takes_its_instrument_s_place() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": "Keys", "id": "fm-synth" })).unwrap();
    run(&mut session, "device.group", json!({ "track": "Keys", "device": 0, "name": "Layer" })).unwrap();
    run(&mut session, "track.add", json!({ "kind": "midi", "name": "Lead" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "fm-synth" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "delay" })).unwrap();
    let chain = |s: &Session, name: &str| {
        let p = s.project();
        let channel = p.tracks.iter().find(|t| t.name == name).and_then(|t| t.channel).unwrap();
        p.channel(channel).unwrap().devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    };
    let lead = chain(&session, "Lead");
    // A group whose branches start with an instrument is an instrument: it goes where Lead's
    // instrument was, before its effects, and takes its place.
    run(&mut session, "device.moveToTrack", json!({ "track": "Keys", "device": 0, "toTrack": "Lead" })).unwrap();
    let moved = chain(&session, "Lead");
    assert_eq!(moved.len(), lead.len(), "{moved:?}");
    assert_eq!(moved.first().map(String::as_str), Some("Layer"));
    assert_eq!(moved.last(), lead.last(), "the effect stays after it");
    // The group is the track's instrument for an effect added after it too: none goes before it.
    let before = run(&mut session, "device.add", json!({ "track": "Lead", "id": "utility", "position": 0 }));
    assert!(before.is_err(), "{before:?}");
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "utility", "position": 1 })).unwrap();
    assert_eq!(chain(&session, "Lead").first().map(String::as_str), Some("Layer"));
}

/// A track plays one instrument: one added to a track that has one takes its place, with the
/// effects after it kept, as one undo step; built-in and plug-in instruments alike.
#[test]
fn an_added_instrument_replaces_the_one_on_the_track() {
    let names = |session: &Session| -> Vec<String> {
        session.project().channels[0].devices.iter().map(|d| d.name.clone()).collect()
    };
    let mut session = Session::new(project());
    session.set_plugin_env(plugin_env());
    run(&mut session, "device.add", json!({ "track": 1, "id": "pitch" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "sampler" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    assert_eq!(names(&session), ["Pitch", "Sampler", "Utility"]);

    let synth = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Synth" })).unwrap();
    assert_eq!((synth["device"].as_u64(), synth["replaced"].as_str()), (Some(1), Some("Sampler")));
    assert_eq!(names(&session), ["Pitch", "Synth", "Utility"]);

    let fm = run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth" })).unwrap();
    assert_eq!(fm["replaced"], "Synth");
    assert_eq!(names(&session), ["Pitch", "FM Synth", "Utility"]);

    // An effect replaces nothing.
    let verb = run(&mut session, "device.addPlugin", json!({ "track": 1, "plugin": "Verb" })).unwrap();
    assert!(verb["replaced"].is_null());
    assert_eq!(names(&session), ["Pitch", "FM Synth", "Utility", "Verb"]);

    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session), ["Pitch", "Synth", "Utility"]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session), ["Pitch", "Sampler", "Utility"]);
}

/// What a built-in device's old `.sd` wrapper looked like: one `native_device` node between the
/// input and the output.
const UTILITY_WRAPPER: &str = r#"{"format":"suhina-device","version":4,
  "device":{"id":"utility","name":"Utility","role":"effect","parameters":[]},
  "nodes":[{"id":"in","kind":"audio_input"},{"id":"device","kind":"native_device","device":"utility"},
           {"id":"out","kind":"audio_output"}],
  "connections":[{"from":"in","to":"device"},{"from":"device","to":"out"}]}"#;

#[test]
fn a_wrapper_file_is_no_longer_a_device_to_add() {
    let path = std::env::temp_dir().join(format!("suhina-wrapper-{}.sd", std::process::id()));
    std::fs::write(&path, UTILITY_WRAPPER).unwrap();
    let mut session = Session::new(project());
    let refused = run(&mut session, "device.addSd", json!({ "track": 1, "path": path }));
    std::fs::remove_file(&path).unwrap();
    assert!(refused.is_err(), "{refused:?}");
    assert!(session.project().channels[0].devices.is_empty());
}
