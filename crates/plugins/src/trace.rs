// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A diagnostic log for development (`docs/debugging.md`): with `suhina --trace <file>`, or
//! `SUHINA_TRACE` set to a file path, the app and its plug-in hosts (which are started with the
//! same path) append one line per event to that file, each with the time since the Unix epoch in
//! milliseconds and the process id. Without either nothing is done, and a trace point costs one
//! check.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The variable that names the trace file; plug-in hosts are started with it set.
pub const VARIABLE: &str = "SUHINA_TRACE";

/// A path given before the first trace point (`start`), over the variable.
static CHOSEN: Mutex<Option<PathBuf>> = Mutex::new(None);

fn file() -> Option<&'static (PathBuf, Mutex<std::fs::File>)> {
    static FILE: OnceLock<Option<(PathBuf, Mutex<std::fs::File>)>> = OnceLock::new();
    FILE.get_or_init(|| {
        let chosen = CHOSEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let path = chosen.or_else(|| std::env::var_os(VARIABLE).map(PathBuf::from))?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok()?;
        Some((path, Mutex::new(file)))
    })
    .as_ref()
}

/// Traces to `path` from now on (`suhina --trace`); call it before anything is traced. The
/// plug-in hosts started after it trace to the same file.
pub fn start(path: &Path) {
    *CHOSEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path.to_path_buf());
    let _ = file();
}

/// Whether tracing is on.
pub fn on() -> bool {
    file().is_some()
}

/// The trace file, when tracing is on: the plug-in hosts are started with it.
pub fn path() -> Option<&'static Path> {
    file().map(|(path, _)| path.as_path())
}

/// Appends `line` to the trace file, when tracing is on.
pub fn line(line: std::fmt::Arguments<'_>) {
    let Some((_, file)) = file() else { return };
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    // One write a line: the app and its hosts append to the same file, and a line written in
    // pieces was cut up by the others' lines.
    let text = format!(
        "{ms} [{}] {line}
",
        std::process::id()
    );
    let mut file = file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = file.write_all(text.as_bytes());
}

/// `trace!("…", args)`: a line in the trace file, when tracing is on.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::trace::on() {
            $crate::trace::line(format_args!($($arg)*));
        }
    };
}

/// Work that takes this long or longer is written by [`timed`].
pub const SLOW_WORK: std::time::Duration = std::time::Duration::from_millis(5);

/// Runs `work`; with tracing on, writes `slow <what>: N ms` when it took [`SLOW_WORK`] or
/// longer, so a part of the window that lags (the library, the sample map) names itself.
pub fn timed<T>(what: &str, work: impl FnOnce() -> T) -> T {
    if !on() {
        return work();
    }
    let started = std::time::Instant::now();
    let out = work();
    let spent = started.elapsed();
    if spent >= SLOW_WORK {
        line(format_args!("slow {what}: {:.1} ms", spent.as_secs_f64() * 1000.0));
    }
    out
}

/// Appends `key: value` when it differs from the last value traced for `key`: a state that is
/// worked out every frame shows only when it changes.
pub fn state(key: &str, value: String) {
    use std::collections::HashMap;
    static LAST: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    if !on() {
        return;
    }
    let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = last.get_or_insert_with(HashMap::new);
    if map.get(key) != Some(&value) {
        line(format_args!("{key}: {value}"));
        map.insert(key.to_string(), value);
    }
}
