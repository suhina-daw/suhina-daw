// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The other Suhina apps running on this machine, for the trace (#1163): a second build playing
//! on the same audio device can make playback scratch with no counter showing it, and builds in
//! different folders do not share the single-instance lock, so the trace names them as the
//! audio device opens.

use std::ffi::OsStr;
use std::path::Path;

/// The other Suhina apps running now, as the trace says it: each one's process id and
/// executable, or `none`. Lists every process, so it is called only when the output opens.
pub(crate) fn describe() -> String {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let Ok(mut system) = System::new() else { return "unknown".into() };
    let kind = ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let all: Vec<(u32, &OsStr, Option<&Path>)> =
        system.processes().iter().map(|(pid, process)| (pid.as_u32(), process.name(), process.exe())).collect();
    others_of(&all, std::process::id())
}

/// The trace's text for the apps among `processes` (process id, name, executable), this process
/// `me` left out.
fn others_of(processes: &[(u32, &OsStr, Option<&Path>)], me: u32) -> String {
    let others: Vec<(u32, Option<&Path>)> = processes
        .iter()
        .filter(|(pid, name, _)| *pid != me && is_app(name))
        .map(|(pid, _, exe)| (*pid, *exe))
        .collect();
    listed(&others)
}

/// Whether a process named `name` is the desktop app (not a plug-in host or the CLI).
fn is_app(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_lowercase();
    name == "suhina" || name == "suhina.exe"
}

/// The trace's text for `others`, sorted by process id.
fn listed(others: &[(u32, Option<&Path>)]) -> String {
    if others.is_empty() {
        return "none".into();
    }
    let mut others = others.to_vec();
    others.sort_by_key(|(pid, _)| *pid);
    let each: Vec<String> = others
        .iter()
        .map(|(pid, exe)| format!("{pid} {}", exe.map_or_else(|| "?".into(), |p| p.display().to_string())))
        .collect();
    each.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_app_counts_and_the_list_reads_by_process() {
        assert!(is_app(OsStr::new("suhina")) && is_app(OsStr::new("Suhina.exe")));
        assert!(!is_app(OsStr::new("suhina-plugin-host")) && !is_app(OsStr::new("suhina-cli.exe")));
        assert_eq!(listed(&[]), "none");
        let (a, b) = (Path::new("/a/suhina"), Path::new("/b/suhina"));
        assert_eq!(listed(&[(20, Some(b)), (7, Some(a)), (9, None)]), "7 /a/suhina; 9 ?; 20 /b/suhina");
    }

    /// This process is not listed even when it is the app, and a process whose id only ends like its
    /// own is.
    #[test]
    fn this_process_is_not_listed() {
        let app = Path::new("/a/suhina");
        let processes = [
            (42, OsStr::new("suhina"), Some(app)),
            (142, OsStr::new("suhina"), Some(app)),
            (43, OsStr::new("suhina-plugin-host"), None),
        ];
        assert_eq!(others_of(&processes, 42), "142 /a/suhina");
        assert_eq!(others_of(&processes[..1], 42), "none");
    }
}
