// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every listed built-in device passes the device conformance checks, as the registry makes it,
//! so a device crate that leaves its own check out is still caught.

use std::sync::Arc;

use suhina_device_api::{Device, DeviceError, Role, conformance};
use suhina_device_registry::{DEVICES, create, create_with, load_builtin_file, pack_files};

/// `device` with a second of a quiet 220 Hz sine loaded when it is an instrument that plays
/// samples (the others ignore it), so that the note checks hear it.
fn with_test_sample(mut device: Box<dyn Device>) -> Box<dyn Device> {
    if device.info().role == Role::Instrument {
        let sine: Arc<[f32]> =
            (0..48_000).map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / 48_000.0).sin() * 0.2).collect();
        if let Some(samples) = device.samples() {
            samples.load_sample(48_000, 1, sine);
        }
    }
    device
}

#[test]
fn every_listed_device_is_made_by_its_id() {
    for (i, info) in DEVICES.iter().enumerate() {
        assert!(DEVICES.iter().take(i).all(|other| other.id != info.id), "{} is listed twice", info.id);
        let device = create(&info.id, 48_000.0).unwrap_or_else(|| panic!("{} is listed but not made", info.id));
        assert_eq!(device.info(), *info);
    }
    assert!(create("no-such-device", 48_000.0).is_none());
}

/// Makes device `id` for a rate as the engine does, with a test sample for an instrument.
fn made(id: &str, rate: f64) -> Result<Box<dyn Device>, DeviceError> {
    create(id, rate).map(with_test_sample).ok_or(DeviceError::InvalidSampleRate(rate))
}

#[test]
fn every_listed_device_passes_the_conformance_checks() {
    let failures: Vec<(&str, Vec<String>)> = std::thread::scope(|scope| {
        let runs: Vec<_> = DEVICES
            .iter()
            .map(|info| scope.spawn(move || (&*info.id, conformance::check(&|rate| made(&info.id, rate)))))
            .collect();
        runs.into_iter().map(|run| run.join().unwrap()).filter(|(_, failures)| !failures.is_empty()).collect()
    });
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The devices that run a file pass the checks with a shipped one loaded: an amp model with a
/// receptive field of thousands of frames and a cabinet's impulse response, so `reset` and a
/// device made at another rate are heard through the model's and the convolution's memory.
#[test]
fn the_file_devices_pass_the_conformance_checks_with_a_shipped_file() {
    let failures: Vec<(&str, Vec<String>)> = std::thread::scope(|scope| {
        let runs: Vec<_> = ["nam-amp-modeler", "nam-cab-loader"]
            .into_iter()
            .map(|id| {
                scope.spawn(move || {
                    let (name, _) = pack_files(id).into_iter().next().unwrap();
                    let asset = load_builtin_file(id, name, 48_000).unwrap();
                    let make = |rate| create_with(id, rate, Some(&asset)).ok_or(DeviceError::InvalidSampleRate(rate));
                    (id, conformance::check(&make))
                })
            })
            .collect();
        runs.into_iter().map(|run| run.join().unwrap()).filter(|(_, failures)| !failures.is_empty()).collect()
    });
    assert!(failures.is_empty(), "{failures:#?}");
}
