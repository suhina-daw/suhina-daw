// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A diagnostic log for development: with `SUHINA_TRACE` set to a file path, the app and its
//! plug-in hosts (which inherit the variable) append one line per event to that file, each with
//! the time since the Unix epoch in milliseconds and the process id. Without it nothing is done.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

fn file() -> Option<&'static Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os("SUHINA_TRACE")?;
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
    })
    .as_ref()
}

/// Whether tracing is on.
pub fn on() -> bool {
    file().is_some()
}

/// Appends `line` to the trace file, when tracing is on.
pub fn line(line: std::fmt::Arguments<'_>) {
    let Some(file) = file() else { return };
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let mut file = file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = writeln!(file, "{ms} [{}] {line}", std::process::id());
}

/// `trace!("…", args)`: a line in the `SUHINA_TRACE` file, when it is set.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::trace::on() {
            $crate::trace::line(format_args!($($arg)*));
        }
    };
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
