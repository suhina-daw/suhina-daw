// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Actual binary tests. Run with scripts/device_exports.py test after packaging the CDylib.
#![cfg(feature = "device-exports")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test setup helpers

use std::path::PathBuf;
use std::time::Duration;
use suhina_device_api::{NoteEvent, ProcessContext, Role};
use suhina_device_exports::exports::{Parameters, exportable, state};
use suhina_plugins::{Event, Launch, offline, scan};

fn artifact(format: &str) -> PathBuf {
    let variable = if format == "clap" { "SUHINA_TEST_EXPORT_CLAP" } else { "SUHINA_TEST_EXPORT_VST3" };
    PathBuf::from(std::env::var_os(variable).expect("run scripts/device_exports.py test"))
}
fn host() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))
}
fn launch(spec: String) -> Launch {
    Launch {
        host: host(),
        plugin: spec,
        channels: 2,
        max_frames: 512,
        sample_rate: 48_000.0,
        timeout: Duration::from_secs(30),
        group: None,
        ara: None,
    }
}

#[test]
#[ignore = "requires packaged export binary; scripts/device_exports.py test"]
fn real_clap_factory_lists_exactly_the_supported_registry_devices() {
    let scan::Scanned::Plugins(plugins) =
        scan::scan_file(&host(), &artifact("clap").display().to_string(), Duration::from_secs(30))
    else {
        panic!("export did not scan")
    };
    let expected: Vec<String> = suhina_device_registry::DEVICES
        .iter()
        .filter(|info| exportable(info))
        .map(|info| format!("org.suhina.device.{}", info.id))
        .collect();
    assert_eq!(plugins.iter().map(|plugin| plugin.id.clone()).collect::<Vec<_>>(), expected);
}

#[test]
#[ignore = "requires packaged export binary; scripts/device_exports.py test"]
fn real_clap_devices_render_like_stock_with_notes_latency_and_saved_parameters() {
    verify_render("clap");
}

#[test]
#[ignore = "requires packaged export binary; scripts/device_exports.py test"]
fn real_vst3_devices_render_like_stock_with_notes_latency_and_saved_parameters() {
    verify_render("vst3");
}

#[test]
#[ignore = "requires packaged export binary; scripts/device_exports.py test"]
fn real_format_adapters_follow_nondefault_transport_for_synced_dsp() {
    let info = suhina_device_registry::DEVICES.iter().find(|info| info.id == "auto-pan").unwrap();
    for format in ["clap", "vst3"] {
        let scan::Scanned::Plugins(plugins) =
            scan::scan_file(&host(), &artifact(format).display().to_string(), Duration::from_secs(30))
        else {
            panic!("scan failed")
        };
        let plugin = plugins.iter().find(|plugin| plugin.name == format!("Suhina {}", info.name)).unwrap();
        let parameters = Parameters::new(info).unwrap();
        parameters.set(suhina_device_exports::exports::parameter_id("sync"), 1.0);
        let context = ProcessContext { tempo: 137.0, beat: 7.25, playing: true };
        let input = vec![0.3; 1024];
        let mut expected = input.clone();
        let mut stock = suhina_device_registry::create(&info.id, 48_000.0).unwrap();
        for index in 0..info.params.len() {
            stock.set_param(index, parameters.value(index).unwrap());
        }
        stock.reset();
        stock.process(&context, &mut expected);
        let events = [Event::transport(true, 0), Event::tempo(137.0), Event::beat(7.25)];
        let rendered = offline::render_with_events(
            launch(plugin.spec()),
            Some(state::save(&parameters).into()),
            &input,
            &events,
            0,
        )
        .unwrap();
        assert_eq!(rendered.state_refused, None);
        assert_eq!(rendered.samples, expected, "{format} nondefault transport");
    }
}

fn verify_render(format: &str) {
    let scanned = scan::scan_file(&host(), &artifact(format).display().to_string(), Duration::from_secs(30));
    let scan::Scanned::Plugins(plugins) = scanned else { panic!("{format} did not scan: {scanned:?}") };
    assert_eq!(plugins.len(), suhina_device_registry::DEVICES.iter().filter(|info| exportable(info)).count());
    for info in suhina_device_registry::DEVICES.iter().filter(|info| exportable(info)) {
        let plugin = plugins.iter().find(|plugin| plugin.name == format!("Suhina {}", info.name)).unwrap();
        let parameters = Parameters::new(info).unwrap();
        // Exercise a non-default stable-id state through the actual host stream.
        if let Some(parameter) = info.params.first() {
            parameters.set(parameters.id(0).unwrap(), parameter.from_normalized(0.37));
        }
        let state = state::save(&parameters);
        let mut stock = suhina_device_registry::create(&info.id, 48_000.0).unwrap();
        for index in 0..info.params.len() {
            stock.set_param(index, parameters.value(index).unwrap());
        }
        stock.reset();
        let frames = 1024;
        let input: Vec<f32> = (0..frames * 2).map(|i| (i as f32 * 0.041).sin() * 0.12).collect();
        let mut expected = input.clone();
        expected.resize((frames + stock.latency() as usize) * 2, 0.0);
        let events = if info.role == Role::Instrument {
            vec![Event::note_on(37, 0, 60, 0.8), Event::note_off(300, 0, 60, 0.4)]
        } else {
            Vec::new()
        };
        let context = ProcessContext::default();
        stock.set_context(&context);
        if info.role == Role::Instrument {
            stock.process(&context, &mut expected[..74]);
            stock.event(NoteEvent::On { key: 60, velocity: 0.8 });
            stock.process(&context, &mut expected[74..600]);
            stock.event(NoteEvent::Off { key: 60, velocity: 0.4 });
            stock.process(&context, &mut expected[600..]);
        } else {
            stock.process(&context, &mut expected);
        }
        let rendered =
            offline::render_with_events(launch(plugin.spec()), Some(state.into()), &input, &events, 0).unwrap();
        assert_eq!(rendered.state_refused, None, "{} state", info.id);
        assert_eq!(rendered.latency, stock.latency(), "{} latency", info.id);
        assert_eq!(rendered.samples, expected[stock.latency() as usize * 2..], "{} render", info.id);
    }
}
