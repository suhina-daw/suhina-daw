// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The real preset codec preserves a canonical profile and rejects ambiguous resources.
#![allow(clippy::unwrap_used)] // deterministic fixture constructors
use serde_json::{Value, json};
use suhina_device_registry::{
    InstrumentPreset, ModalProfile, PresetSample, PresetSourceScope, preset, write_instrument_preset,
};
use suhina_dsp::modal::profile::ProfileMode;

fn state() -> InstrumentPreset {
    InstrumentPreset {
        profile: Some(
            ModalProfile::new(
                60,
                262.7,
                &[
                    ProfileMode { ratio: 1.0, magnitude: 0.8, decay_seconds: 1.3 },
                    ProfileMode { ratio: 2.756, magnitude: 0.2, decay_seconds: 0.4 },
                ],
            )
            .unwrap(),
        ),
        sample: Some(PresetSample {
            scope: PresetSourceScope::Preset,
            path: "Bell.assets/abc/owner.wav".into(),
            sha256: [17; 32],
            bytes: 1024,
        }),
    }
}

fn document() -> Value {
    serde_json::from_slice(
        &write_instrument_preset("Bell", &[("model_revision", 2.0), ("source", 2.0)], &state()).unwrap(),
    )
    .unwrap()
}

#[test]
fn canonical_profile_and_both_source_scopes_round_trip() {
    for scope in [PresetSourceScope::Preset, PresetSourceScope::Library] {
        let mut state = state();
        state.sample.as_mut().unwrap().scope = scope;
        let encoded = write_instrument_preset("Bell", &[("model_revision", 2.0), ("source", 2.0)], &state).unwrap();
        let read = preset(&encoded, 1).unwrap();
        assert_eq!(read.instrument.as_ref(), Some(&state));
        assert_eq!(read.device, "instrument");
        assert!(read.params.contains(&("source", 2.0)));
        assert_eq!(
            write_instrument_preset(&read.name, &read.params, read.instrument.as_ref().unwrap()).unwrap(),
            encoded
        );
    }
}

#[test]
fn unknown_or_malformed_state_never_turns_into_a_default_instrument() {
    for (pointer, bad) in [
        ("/version", json!(3)),
        ("/version", json!(1)),
        ("/params/model_revision", json!(1)),
        ("/params/model_revision", json!(99)),
        ("/instrument", Value::Null),
        ("/instrument/profile", json!([1, 2, 3])),
        ("/instrument/profile/0", json!(256)),
        ("/instrument/profile/9", json!(255)),
        ("/instrument/sample/scope", json!("absolute")),
        ("/instrument/sample/sha256", json!("abc")),
        ("/instrument/sample/sha256", json!("F".repeat(64))),
        ("/instrument/sample/bytes", json!(0)),
        ("/instrument/sample/bytes", json!(128 * 1024 * 1024 + 1)),
        ("/instrument/sample/path", json!("../outside.wav")),
        ("/instrument/sample/path", json!("C:/outside.wav")),
        ("/instrument/sample/path", json!("a\\outside.wav")),
        ("/instrument/sample/path", json!("/outside.wav")),
        ("/instrument/sample/path", json!("a/CON.wav")),
        ("/instrument/sample/path", json!("a/./b.wav")),
    ] {
        let mut value = document();
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(preset(&serde_json::to_vec(&value).unwrap(), 1).is_err(), "{pointer}: {value}");
    }
    let mut missing = document();
    missing["instrument"].as_object_mut().unwrap().remove("sample");
    assert!(preset(&serde_json::to_vec(&missing).unwrap(), 1).is_err());
    assert!(preset(&vec![b' '; 65537], 1).is_err());
}

#[test]
fn legacy_parameters_and_nam_files_remain_version_one() {
    for (device, file) in [("instrument", None), ("nam-amp-modeler", Some("Models/own.nam"))] {
        let encoded = suhina_device_registry::write_preset_file(device, "Own", file, &[]).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap()["version"], 1);
        let read = preset(&encoded, 1).unwrap();
        assert!(read.instrument.is_none());
        assert_eq!(read.file.as_deref(), file);
    }
}
