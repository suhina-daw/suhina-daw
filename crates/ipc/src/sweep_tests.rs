// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// A folder of its own for one test, removed when the test ends, also when it fails.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("suhina-ipc-sweep-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, [0u8; 64]).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn only_the_names_this_crate_makes_are_read() {
    assert_eq!(parse("suhina-ipc-12@345-0"), Some(Name::Owned { pid: 12, start: 345 }));
    assert_eq!(parse("suhina-ara-12@0-3"), Some(Name::Owned { pid: 12, start: 0 }));
    assert_eq!(parse("suhina-ipc-12-0-345"), Some(Name::Earlier { pid: 12 }));
    assert_eq!(parse("suhina-ara-12-3-0"), Some(Name::Earlier { pid: 12 }));
    assert_eq!(parse("suhina-ipc-12.lock"), Some(Name::Earlier { pid: 12 }));
    assert_eq!(parse("suhina-ipc-12.locking"), Some(Name::Earlier { pid: 12 }));
    for other in [
        "suhina-ipc-short-12",
        "suhina-ipc-12-0",
        "suhina-ipc-12-0-1-2",
        "suhina-ipc-12-0-x",
        "suhina-ipc--0-1",
        "suhina-ipc-+12-0-1",
        "suhina-ipc-99999999999-0-1",
        "suhina-ipc-12@345",
        "suhina-ipc-12@-0",
        "suhina-ipc-12@3-x",
        "suhina-ipc-@3-0",
        "suhina-ara-12.lock",
        "suhina-ipc-x.lock",
        "suhina-ipc-12.lock.old",
        "suhina-ipc-sweep-12-0",
        "suhina-relink-12",
        "notes.txt",
    ] {
        assert_eq!(parse(other), None, "{other}");
    }
}

#[test]
fn files_of_gone_processes_go_and_others_stay() {
    let dir = Scratch::new();
    let gone_owned = dir.file("suhina-ipc-7@100-0");
    let gone_earlier = dir.file("suhina-ara-8-0-1");
    let live = [dir.file("suhina-ipc-9@200-0"), dir.file("suhina-ara-9@200-1")];
    let other = dir.file("notes.txt");
    let mut asked = Vec::new();
    sweep(&dir.0, 1, &mut |name, own| {
        asked.push(name);
        assert_eq!(own, 1);
        !matches!(name, Name::Owned { pid: 9, .. })
    });
    // Asked once a name, however many files it has.
    asked.sort();
    assert_eq!(asked.len(), 3, "{asked:?}");
    assert!(!gone_owned.exists() && !gone_earlier.exists());
    assert!(live.iter().all(|f| f.exists()) && other.exists());
}

#[test]
fn this_process_keeps_its_own_files_and_loses_an_earlier_versions() {
    let own = std::process::id();
    let start = started(own);
    #[cfg(target_os = "macos")]
    assert_ne!(start, 0);
    assert!(!gone(Name::Owned { pid: own, start }, own));
    // A file of an earlier version named after this pid is from a gone process that had it.
    assert!(gone(Name::Earlier { pid: own }, own));
    // The same pid with another start time is a process that had it before.
    #[cfg(target_os = "macos")]
    assert!(gone(Name::Owned { pid: own, start: start + 1 }, own));
}

#[test]
fn a_start_time_reads_back_the_same() {
    let own = std::process::id();
    assert_eq!(started(own), started(own));
}

#[test]
fn a_pid_no_process_has_is_gone() {
    // Above the largest pid the systems this runs on give out.
    let unused = 4_000_001;
    assert!(gone(Name::Owned { pid: unused, start: 5 }, 1));
    assert!(gone(Name::Earlier { pid: unused }, 1));
}

#[test]
fn a_process_with_many_files_is_asked_about_once() {
    let dir = Scratch::new();
    for n in 0..20 {
        dir.file(&format!("suhina-ipc-7@100-{n}"));
    }
    let mut calls = 0;
    sweep(&dir.0, 1, &mut |_, _| {
        calls += 1;
        true
    });
    assert_eq!(calls, 1);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}
