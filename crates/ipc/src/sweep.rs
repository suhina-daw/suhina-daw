// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Removing shared-memory files that a crash or a kill left in the temporary folder (#805).
//!
//! On Windows and Linux the shared memory has no file ([`crate::shm`]), so nothing is left.
//! On macOS a file has a name from when it is made until the host has opened it (a link) or
//! until the engine lets go of it (an audio source, which a restarted host opens again). Its name
//! holds its process's id and that process's start time, `suhina-<kind>-<pid>@<start>-<n>`, and
//! a pid and a start time together name one process for good: the file's process is gone when no
//! process has that pid, or the one that has it started at another time. No lock is needed, and a
//! live process's files are never taken for a gone one's.
//!
//! Earlier versions named their files `suhina-<kind>-<pid>-<n>-<nanos>`, in the temporary folder
//! (`/dev/shm` on Linux) on every system, and left them after a crash; test builds of a first fix
//! left `suhina-ipc-<pid>.lock` files. Those go when no process has their pid; one that reused it
//! keeps them, until it ends.
//!
//! The first shared memory a process makes sweeps once; [`sweep_leftovers`] sweeps again.
//! Nothing here returns an error: a file that cannot be removed is left for the next sweep. Never
//! reachable from the audio callback: it runs where links and sources are made.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Once;

/// Most leftover files one sweep looks at, so a folder full of them costs a bounded start.
const MOST_FILES: usize = 100_000;

/// Sweeps the first time this process makes shared memory.
pub(crate) fn before_making_files() {
    static ONCE: Once = Once::new();
    ONCE.call_once(sweep_leftovers);
}

/// Removes the shared-memory files of Suhina processes that are gone, and never those of a live
/// one. Making the first link or audio source in a process does this already.
pub fn sweep_leftovers() {
    let own = std::process::id();
    for dir in folders() {
        sweep(&dir, own, &mut gone);
    }
}

/// Where this and earlier versions put shared-memory files.
fn folders() -> Vec<PathBuf> {
    let mut folders = vec![std::env::temp_dir()];
    let shm = Path::new("/dev/shm");
    if cfg!(target_os = "linux") && shm.is_dir() {
        folders.push(shm.to_path_buf());
    }
    folders
}

/// Whose a file name in the folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Name {
    /// Made by this version: the process `pid` that started at `start`.
    Owned { pid: u32, start: u64 },
    /// Made by an earlier version, by the process `pid`.
    Earlier { pid: u32 },
}

/// Only names this crate makes; anything else in the folder is never touched.
fn parse(name: &str) -> Option<Name> {
    // The lock files of test builds of the first fix for #805.
    if let Some(pid) = name
        .strip_prefix("suhina-ipc-")
        .and_then(|rest| rest.strip_suffix(".lock").or_else(|| rest.strip_suffix(".locking")).and_then(number))
    {
        return Some(Name::Earlier { pid });
    }
    let rest = name.strip_prefix("suhina-ipc-").or_else(|| name.strip_prefix("suhina-ara-"))?;
    if let Some((pid, rest)) = rest.split_once('@') {
        let (start, n) = rest.split_once('-')?;
        return (digits(n) && digits(start)).then_some(Name::Owned { pid: number(pid)?, start: number(start)? });
    }
    let mut parts = rest.split('-');
    let (pid, n, nanos) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !digits(n) || !digits(nanos) {
        return None;
    }
    Some(Name::Earlier { pid: number(pid)? })
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn number<T: std::str::FromStr>(s: &str) -> Option<T> {
    if digits(s) { s.parse().ok() } else { None }
}

/// Whether the process a file name belongs to is gone. `own` is this process, which makes no
/// file of an earlier version.
fn gone(name: Name, own: u32) -> bool {
    match name {
        // A start of 0 is one its process could not tell when it named the file.
        Name::Owned { pid, start } => match started_at(pid) {
            Lookup::Gone => true,
            Lookup::At(now) if start != 0 => now != start,
            Lookup::At(_) | Lookup::Unknown => !running(pid),
        },
        Name::Earlier { pid } => pid == own || !running(pid),
    }
}

/// Removes the files in `dir` whose process `is_gone` says is gone, asking once a name.
fn sweep(dir: &Path, own: u32, is_gone: &mut impl FnMut(Name, u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: BTreeMap<Name, Vec<PathBuf>> = BTreeMap::new();
    for entry in entries.flatten().take(MOST_FILES) {
        if let Some(name) = entry.file_name().to_str().and_then(parse) {
            files.entry(name).or_default().push(entry.path());
        }
    }
    for (name, paths) in files {
        if is_gone(name, own) {
            for path in paths {
                // Still mapped by a host on Windows: removing it fails, and the next sweep tries
                // again.
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// When a process started, as far as can be told.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Lookup {
    /// No process has the pid.
    Gone,
    /// It started at this many microseconds after the epoch.
    At(u64),
    /// It cannot be told here.
    Unknown,
}

/// This process's start time, for the names of its files; 0 where it cannot be told.
#[cfg_attr(any(windows, target_os = "linux"), allow(dead_code))]
pub(crate) fn started(pid: u32) -> u64 {
    match started_at(pid) {
        Lookup::At(start) => start,
        Lookup::Gone | Lookup::Unknown => 0,
    }
}

#[cfg(target_os = "macos")]
fn started_at(pid: u32) -> Lookup {
    let Ok(pid) = i32::try_from(pid) else { return Lookup::Gone };
    let size = size_of::<libc::proc_bsdinfo>() as i32;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    // SAFETY: `info` is writable and `size` bytes long; PROC_PIDTBSDINFO writes at most that.
    let written = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size) };
    if written == size {
        // SAFETY: proc_pidinfo filled all of it, and every bit pattern is valid for its integers.
        let info = unsafe { info.assume_init() };
        Lookup::At(info.pbi_start_tvsec.saturating_mul(1_000_000).saturating_add(info.pbi_start_tvusec))
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Lookup::Gone
    } else {
        Lookup::Unknown
    }
}

#[cfg(not(target_os = "macos"))]
fn started_at(_pid: u32) -> Lookup {
    Lookup::Unknown
}

/// Whether a process has the pid; `true` when it cannot be told.
#[cfg(unix)]
fn running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 sends nothing; it only checks that the process exists.
    let sent = unsafe { libc::kill(pid, 0) };
    sent == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(windows)]
fn running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: asks for a handle with the least access; nothing is passed by pointer.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // SAFETY: reads this thread's last error, set by the call above.
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    // SAFETY: the handle OpenProcess returned, closed once.
    unsafe { CloseHandle(handle) };
    true
}

#[cfg(not(any(unix, windows)))]
fn running(_pid: u32) -> bool {
    true
}

#[cfg(test)]
#[path = "sweep_tests.rs"]
mod tests;
