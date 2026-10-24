// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every listed built-in device passes the device conformance checks, as the registry makes it,
//! so a device crate that leaves its own check out is still caught.

use std::sync::Arc;

use suhina_device_api::{Device, Role, conformance};
use suhina_device_registry::{DEVICES, create};

/// `device` with a second of a quiet 220 Hz sine loaded when it is an instrument that plays
/// samples (the others ignore it), so that the note checks hear it.
fn with_test_sample(mut device: Box<dyn Device>) -> Box<dyn Device> {
    if device.info().role == Role::Instrument {
        let sine: Arc<[f32]> =
            (0..48_000).map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / 48_000.0).sin() * 0.2).collect();
        device.load_sample(48_000, 1, sine);
    }
    device
}

#[test]
fn every_listed_device_is_made_by_its_id() {
    for (i, info) in DEVICES.iter().enumerate() {
        assert!(DEVICES.iter().take(i).all(|other| other.id != info.id), "{} is listed twice", info.id);
        let device = create(info.id).unwrap_or_else(|| panic!("{} is listed but not made", info.id));
        assert_eq!(device.info(), *info);
    }
    assert!(create("no-such-device").is_none());
}

#[test]
fn every_listed_device_passes_the_conformance_checks() {
    let failures: Vec<(&str, Vec<String>)> = std::thread::scope(|scope| {
        let runs: Vec<_> = DEVICES
            .iter()
            .map(|info| {
                scope.spawn(move || (info.id, conformance::check(&|| with_test_sample(create(info.id).unwrap()))))
            })
            .collect();
        runs.into_iter().map(|run| run.join().unwrap()).filter(|(_, failures)| !failures.is_empty()).collect()
    });
    assert!(failures.is_empty(), "{failures:#?}");
}
