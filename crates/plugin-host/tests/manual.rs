// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tools for looking into one installed plug-in by hand, run with `--ignored`: they take the
//! plug-in from `SUHINA_PLUGIN` (`vst3:<bundle>` or `clap:<file>`, as the host takes it) and,
//! if set, the host from `SUHINA_HOST` (a program that runs `suhina-plugin-host` with the
//! arguments it is given). With the `vst2` feature, `SUHINA_TEST_VST2` probes an installed
//! VST2 plug-in the same way (ADR 0022).
//!
//! ```sh
//! SUHINA_PLUGIN="vst3:<path to a .vst3 bundle>" \
//!   cargo test -p suhina-plugin-host --test manual -- --ignored --nocapture
//! ```
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::Duration;

use suhina_plugins::{Launch, Plugin};

fn launch() -> Option<Launch> {
    let plugin = std::env::var("SUHINA_PLUGIN").ok()?;
    let host = std::env::var("SUHINA_HOST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")));
    Some(Launch {
        host,
        plugin,
        channels: 2,
        max_frames: 512,
        sample_rate: 48_000.0,
        timeout: Duration::from_secs(120),
        group: None,
        ara: None,
    })
}

/// Loads two instances of the plug-in into one host process, one after the other, and
/// reports how long each took (a second instance that takes longer than the first is the
/// plug-in's doing, not the host's).
#[test]
#[ignore = "needs SUHINA_PLUGIN; looks into one installed plug-in by hand"]
fn two_instances_load_into_one_process() {
    let Some(launch) = launch() else {
        eprintln!("SUHINA_PLUGIN not set; nothing to look into");
        return;
    };
    let launch = Launch { group: Some("manual".into()), ..launch };
    let started = std::time::Instant::now();
    let (first, _first_audio) = Plugin::start(launch.clone()).unwrap_or_else(|e| panic!("first: {e}"));
    eprintln!("first loaded in {:.1} s (latency {})", started.elapsed().as_secs_f64(), first.latency());
    let started = std::time::Instant::now();
    let (second, _second_audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("second: {e}"));
    eprintln!("second loaded in {:.1} s (latency {})", started.elapsed().as_secs_f64(), second.latency());
    assert_eq!(first.process_id(), second.process_id());
}

/// Loads the plug-in, opens its editor in a window of its own, keeps it open for a while and
/// reports whether the host stayed alive.
#[test]
#[ignore = "needs SUHINA_PLUGIN; looks into one installed plug-in by hand"]
fn an_installed_plugins_editor_opens() {
    let Some(launch) = launch() else {
        eprintln!("SUHINA_PLUGIN not set; nothing to look into");
        return;
    };
    let (mut plugin, _audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("start: {e}"));
    eprintln!("loaded, latency {} frames; opening the editor", plugin.latency());
    match plugin.open_editor() {
        Ok(size) => eprintln!("editor open: {size:?}"),
        Err(e) => panic!("open_editor: {e}"),
    }
    let seconds = std::env::var("SUHINA_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(10u64);
    for second in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        let health = plugin.health();
        eprintln!("{second}s: {health:?}, editor open {}", plugin.editor_open());
        assert_eq!(health, suhina_plugins::Health::Running, "the host did not stay alive");
    }
    plugin.close_editor().unwrap();
}

/// `--vst2-probe test:vst2`: the whole probe tool over the built-in mock, from the argument
/// to the printed report, as it runs by hand. A test of the tool, not of the mock: the
/// mock's own half of the conversation is checked in the crate's unit tests.
#[cfg(feature = "vst2")]
#[test]
fn the_vst2_probe_tool_reports_on_the_mock() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_suhina-plugin-host"))
        .arg("--vst2-probe")
        .arg("test:vst2")
        .output()
        .expect("the host process runs");
    assert!(out.status.success(), "the probe on the mock exits well: {}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(report.contains("plug-in: the built-in mock"), "{report}");
    assert!(report.contains("effGetChunk (assumed 23)"), "{report}");
    assert!(report.contains("audioMasterAutomate"), "{report}");
}

/// Runs the VST2 probe over an installed plug-in and prints its report: the observation that
/// settles the interface's *assumed* facts — the entry point that answered, the op-codes the
/// plug-in answers, what it asked of the host, what its chunks hold (ADR 0022 decision 4).
/// Run it with `--nocapture`; the report is the result.
///
/// ```sh
/// SUHINA_TEST_VST2="/path/to/a/vst2 plug.vst" \
///   cargo test -p suhina-plugin-host --features vst2 --test manual -- --ignored --nocapture
/// ```
///
/// The plug-in runs in a disposable process, as the scanner's plug-ins do: a plug-in that
/// crashes the probe fails this test with the process's exit, which is itself an observation.
#[cfg(feature = "vst2")]
#[test]
#[ignore = "needs SUHINA_TEST_VST2; settles the spec's assumed facts against an installed plug-in"]
fn an_installed_vst2_plug_in_answers_the_probe() {
    let Some(path) = std::env::var_os("SUHINA_TEST_VST2") else {
        eprintln!("SUHINA_TEST_VST2 not set; nothing to probe");
        return;
    };
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_suhina-plugin-host"))
        .arg("--vst2-probe")
        .arg(&path)
        .output()
        .unwrap_or_else(|e| panic!("could not run the probe: {e}"));
    println!("{}", String::from_utf8_lossy(&out.stdout));
    assert!(out.status.success(), "the probe did not finish: {}", String::from_utf8_lossy(&out.stderr));
}
