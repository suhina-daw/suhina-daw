// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host reports its process's resident memory to the engine once a second, so the control
//! bar can add every host's to the program's own RAM (< descendants of #550). The strong proofs
//! of taking a report in and letting a dead host's go are unit tests on the plugins crate; this
//! checks the real host sends it and the readout stays sane.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use suhina_ipc::Outcome;
use suhina_plugins::{Health, Launch, Plugin, host_memory};

const PATIENT: Duration = Duration::from_secs(5);

fn launch(group: &str) -> Launch {
    Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: "test:gain".into(),
        channels: 2,
        max_frames: 64,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: Some(group.into()),
        ara: None,
    }
}

/// The reported numbers stay in what a resident process can be: not nothing, not more than a
/// machine's RAM could plausibly be for one host.
fn sane(bytes: u64) -> bool {
    (1024..=16 * 1024 * 1024 * 1024).contains(&bytes)
}

/// Whether a process with id `pid` runs.
fn process_runs(pid: u32) -> bool {
    if cfg!(windows) {
        let filter = format!("PID eq {pid}");
        let output = std::process::Command::new("tasklist").args(["/FI", &filter, "/NH", "/FO", "CSV"]).output();
        output.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    } else if cfg!(target_os = "macos") {
        // No `/proc` here; `ps` says whether a process with this id exists at all.
        std::process::Command::new("ps").args(["-p", &pid.to_string()]).output().is_ok_and(|o| o.status.success())
    } else {
        PathBuf::from(format!("/proc/{pid}")).exists()
    }
}

#[test]
fn the_control_bar_reads_what_the_host_reports_and_drops_what_died() {
    // Another test may hold hosts while this one runs; the ones it saw at the start decide
    // whether the end could be an empty read.
    let alone_at_the_start = host_memory().is_empty();
    let (mut plugin, mut audio) = Plugin::start(launch("memory-report")).unwrap_or_else(|e| panic!("start: {e}"));
    let pid = plugin.process_id();

    // The first report lands within about a second of the host starting; the wait is three
    // times the cadence, so a busy runner cannot flake the test.
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && host_memory().is_empty() {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!host_memory().is_empty(), "no host reported its memory within three seconds");
    for bytes in host_memory() {
        assert!(sane(bytes), "the control bar would show {bytes} bytes");
    }
    assert_eq!(plugin.health(), Health::Running, "the host stayed alive");
    let mut output = [7.0f32; 2 * 64];
    // The launch timeout is what an integration test may wait: a busy runner must not make a
    // healthy host miss.
    let outcome = audio.process(&[1.0; 64], &mut output, PATIENT);
    assert!(matches!(outcome, Outcome::Done | Outcome::Busy), "{outcome:?}");

    // Dropping the last instance ends the process, which stops being counted.
    drop(audio);
    drop(plugin);
    let deadline = Instant::now() + PATIENT;
    while process_runs(pid) {
        assert!(Instant::now() < deadline, "the process outlived its last instance");
        std::thread::sleep(Duration::from_millis(20));
    }
    if alone_at_the_start {
        assert!(host_memory().is_empty(), "a dead host's report must not be counted");
    }
}
