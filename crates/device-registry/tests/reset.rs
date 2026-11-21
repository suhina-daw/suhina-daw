// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `reset` allocates and frees nothing in any listed device (ADR 0004 › Amendment: a device is
//! made at its rate): it runs on the audio thread like `process`. Each device is made as the
//! engine makes it, at 48 and 44.1 kHz, run with its parameters at their defaults, their
//! minimums and their maximums, and the devices that run a file with a shipped one. The counter
//! counts this thread's allocations, in numbers, not time (AGENTS.md rule 7).

use std::sync::Arc;

use suhina_device_api::{Device, NoteEvent, ProcessContext, Role, conformance};
use suhina_device_registry::{DEVICES, create, create_with, load_builtin_file, pack_files};

/// What `reset` allocated or freed in `device`, after running it with every parameter at
/// `value_of` its range: `None` when nothing.
fn reset_allocations(device: &mut dyn Device, value_of: ValueOf) -> Option<String> {
    let params = device.info().params.clone();
    for (index, param) in params.iter().enumerate() {
        device.set_param(index, value_of(param.min, param.max, param.default));
    }
    if device.info().role == Role::Instrument {
        device.event(NoteEvent::On { key: 60, velocity: 0.8 });
    }
    let mut signal = conformance::test_signal(2_048);
    for block in signal.chunks_mut(512) {
        device.process(&ProcessContext::default(), block);
    }
    let counted = allocation_counter::measure(|| device.reset());
    (counted.count_total != 0 || counted.count_current != 0)
        .then(|| format!("{} allocations, {} still held after", counted.count_total, counted.count_current))
}

/// A parameter's value from its minimum, maximum and default.
type ValueOf = fn(f64, f64, f64) -> f64;

/// Every way a device is run before its `reset` is measured.
const SETTINGS: [(&str, ValueOf); 3] =
    [("defaults", |_, _, default| default), ("minimums", |min, _, _| min), ("maximums", |_, max, _| max)];

/// The failures of `make`'s device, as `what`, at each rate and setting.
fn check(what: &str, make: &dyn Fn(f64) -> Option<Box<dyn Device>>, failures: &mut Vec<String>) {
    for rate in [48_000.0, 44_100.0] {
        for (setting, value_of) in SETTINGS {
            let Some(mut device) = make(rate) else {
                failures.push(format!("{what} is not made at {rate} Hz"));
                continue;
            };
            if let Some(found) = reset_allocations(device.as_mut(), value_of) {
                failures.push(format!("{what} at {rate} Hz, {setting}: reset made {found}"));
            }
        }
    }
}

/// `device` with a second of a quiet sine loaded when it plays samples.
fn with_test_sample(mut device: Box<dyn Device>) -> Box<dyn Device> {
    let sine: Arc<[f32]> =
        (0..48_000).map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / 48_000.0).sin() * 0.2).collect();
    if let Some(samples) = device.samples() {
        samples.load_sample(48_000, 1, sine);
    }
    device
}

#[test]
fn the_counter_counts_this_threads_allocations() {
    let counted = allocation_counter::measure(|| {
        std::hint::black_box(vec![1u8; 128]);
    });
    assert!(counted.count_total > 0);
}

#[test]
fn instrument_render_events_and_reset_allocate_and_free_nothing() {
    for model in 0..3 {
        let mut instrument = with_test_sample(create("instrument", 48000.0).unwrap());
        if model == 2 {
            // A fitted set with a held layer: vibrato, drift, onsets, noise and a release.
            use suhina_dsp::modal::{
                model::{ModelMode, ModelRecording, ModelSet},
                sustain::Sustain,
            };
            let mut layer = Sustain {
                vibrato_hz: 5.5,
                vibrato_cents: 30.0,
                drift_cents: 2.0,
                drift_hz: 2.0,
                release_seconds: 0.4,
                body_fixed: 1.0,
                ..Sustain::default()
            };
            layer.noise = [1e-4; suhina_dsp::modal::sustain::NOISE_BANDS];
            layer.rise[0] = 0.1;
            let recording = ModelRecording {
                key: 60,
                source: 0,
                velocity: 0.8,
                level: 0.5,
                modes: vec![ModelMode::plain(1.0, 0.5, 60.0), ModelMode::plain(2.0, 0.3, 60.0)],
                attack_rate: 0,
                attack: Vec::new(),
                sustain: Some(layer),
                stereo: None,
                tail: None,
                glide: None,
            };
            instrument.load_model_state(&ModelSet::new(vec![recording]).unwrap().encode()).unwrap();
        }
        if model == 1 {
            use suhina_dsp::modal::profile::{ModalProfile, ProfileMode};
            let modes: Vec<_> = (1..=16)
                .map(|index| ProfileMode {
                    ratio: index as f64 * 1.03,
                    magnitude: 1.0 / index as f64,
                    decay_seconds: 0.5,
                })
                .collect();
            let profile = ModalProfile::new(60, 263.1, &modes).unwrap();
            instrument.load_model_state(&profile.encode()).unwrap();
        }
        let source = instrument.info().param_index("source").unwrap();
        let blend = instrument.info().param_index("blend").unwrap();
        let mut buffer = [0.0; 128];
        let counted = allocation_counter::measure(|| {
            for mode in [0.0, 1.0, 2.0] {
                instrument.set_param(source, mode);
                instrument.reset();
                for key in 0..127 {
                    instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
                    instrument.event(NoteEvent::On { key, velocity: 0.8 });
                    instrument.set_param(blend, f64::from(key) / 127.0);
                    instrument.event(NoteEvent::PitchBend(f32::from(key) / 127.0));
                    instrument.process(&ProcessContext::default(), &mut buffer);
                    instrument.event(NoteEvent::Off { key, velocity: 0.0 });
                    instrument.event(NoteEvent::On { key, velocity: 0.8 });
                    instrument.event(NoteEvent::Controller { number: 64, value: 0.0 });
                    instrument.process(&ProcessContext::default(), &mut buffer);
                    instrument.event(NoteEvent::Off { key, velocity: 0.0 });
                }
                instrument.event(NoteEvent::AllNotesOff);
                instrument.reset();
            }
        });
        assert_eq!(counted.count_total, 0, "callback allocated: {counted:?}");
        assert_eq!(counted.count_current, 0, "callback freed existing storage: {counted:?}");
    }
}

#[test]
fn reset_allocates_nothing_in_any_listed_device() {
    let mut failures = Vec::new();
    for info in DEVICES.iter() {
        check(&info.id, &|rate| create(&info.id, rate).map(with_test_sample), &mut failures);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn reset_allocates_nothing_in_a_device_running_a_shipped_file() {
    let mut failures = Vec::new();
    for id in ["nam-amp-modeler", "nam-cab-loader"] {
        let (name, _) = pack_files(id).into_iter().next().unwrap();
        // Read for 48 kHz: at 44.1 kHz the amp's resampler legs run around the model, and the
        // cab loader resamples the impulse response.
        let asset = load_builtin_file(id, name, 48_000).unwrap();
        check(&format!("{id} running {name}"), &|rate| create_with(id, rate, Some(&asset)), &mut failures);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
