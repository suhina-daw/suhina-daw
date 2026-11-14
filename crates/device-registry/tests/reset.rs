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
