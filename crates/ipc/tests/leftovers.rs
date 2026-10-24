// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shared memory of a process that crashed or was killed is freed: by the operating system
//! on Windows and Linux, where it has no file, and by the next process's sweep on macOS. That of
//! a live Suhina process stays readable (#805).
#![allow(clippy::panic)] // test helpers outside #[test] functions

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard};

use suhina_ipc::{AudioSource, Served, sweep_leftovers};

/// One test at a time: a test's sweep would otherwise remove the files another test is looking
/// at, once that test's peer is gone.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A `suhina-ipc-peer` in `mode` (`make` or `idle`), and the lines it printed.
fn peer(mode: &str, lines: usize) -> (Child, Vec<String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_suhina-ipc-peer"))
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("starting the peer: {e}"));
    let stdout: ChildStdout = child.stdout.take().unwrap_or_else(|| panic!("no stdout"));
    let printed: Vec<String> = BufReader::new(stdout).lines().take(lines).map_while(Result::ok).collect();
    assert_eq!(printed.len(), lines, "the peer printed {printed:?}");
    (child, printed)
}

/// Lets a `make` peer end normally, as when Suhina quits.
fn end(mut child: Child) {
    drop(child.stdin.take());
    let _ = child.wait();
}

/// Whether the link and the source at `made` (a `make` peer's two lines) open.
fn opens(made: &[String]) -> (bool, bool) {
    (Served::open(&made[0]).is_ok(), AudioSource::open(&made[1]).is_ok())
}

/// Files of the process `pid` in the folders this or an earlier version put shared memory in.
fn files_of(pid: u32) -> Vec<PathBuf> {
    let mut folders = vec![std::env::temp_dir()];
    if cfg!(target_os = "linux") {
        folders.push(PathBuf::from("/dev/shm"));
    }
    let names = ["ipc", "ara"].map(|kind| [format!("suhina-{kind}-{pid}-"), format!("suhina-{kind}-{pid}@")]);
    folders
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            names.iter().flatten().any(|prefix| name.starts_with(prefix.as_str()))
        })
        .map(|entry| entry.path())
        .collect()
}

#[test]
fn a_killed_processs_memory_is_freed() {
    let _turn = one_at_a_time();
    let (mut child, made) = peer("make", 2);
    let pid = child.id();
    // Live, its memory opens.
    assert_eq!(opens(&made), (true, true));
    let _ = child.kill();
    let _ = child.wait();
    // Nothing removed a file of it, but on Windows and Linux there is none, and the memory went
    // with the process.
    if cfg!(any(windows, target_os = "linux")) {
        assert!(files_of(pid).is_empty(), "{:?}", files_of(pid));
        assert_eq!(opens(&made), (false, false));
    } else {
        assert_eq!(files_of(pid).len(), 2, "{made:?}");
    }
    sweep_leftovers();
    assert!(files_of(pid).is_empty(), "{:?}", files_of(pid));
    assert_eq!(opens(&made), (false, false));
}

#[test]
fn a_live_processs_memory_stays_readable() {
    let _turn = one_at_a_time();
    let (child, made) = peer("make", 2);
    sweep_leftovers();
    // As a second instance starting would, which sweeps too.
    let (other, others) = peer("make", 2);
    let source = AudioSource::open(&made[1]).unwrap_or_else(|e| panic!("{e}"));
    let mut out = [0.0; 4];
    source.read(1, 0, &mut out);
    assert_eq!(out, [0.5; 4]);
    assert!(Served::open(&made[0]).is_ok());
    drop(source);
    // It ends normally and frees its own memory; the other's stays.
    let pid = child.id();
    end(child);
    assert!(files_of(pid).is_empty(), "{:?}", files_of(pid));
    assert_eq!(opens(&made), (false, false));
    assert_eq!(opens(&others), (true, true));
    end(other);
}

#[test]
fn a_pid_in_use_by_another_process_keeps_only_what_could_be_its_own() {
    let _turn = one_at_a_time();
    // A live process that is not a Suhina one.
    let (idle, printed) = peer("idle", 1);
    let pid = idle.id();
    assert_eq!(printed, [pid.to_string()]);
    let dir = std::env::temp_dir();
    // An earlier version's file named after a live pid may be that process's: it stays.
    let earlier = dir.join(format!("suhina-ipc-{pid}-0-1"));
    std::fs::write(&earlier, [0u8; 64]).unwrap_or_else(|e| panic!("{e}"));
    // A file of this version names the start time of its process too, so one of a process that
    // had the pid before goes (where the start time can be told: macOS).
    let before = dir.join(format!("suhina-ara-{pid}@1-0"));
    if cfg!(target_os = "macos") {
        std::fs::write(&before, [0u8; 64]).unwrap_or_else(|e| panic!("{e}"));
    }
    sweep_leftovers();
    assert!(earlier.exists());
    assert!(!before.exists());
    end(idle);
    sweep_leftovers();
    assert!(!earlier.exists());
}
