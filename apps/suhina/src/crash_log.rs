// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A crash leaves a trace: a panic writes what happened to `<data>/crashes/`, besides the usual
//! message on stderr, which nothing shows when Suhina was started from the desktop. The file
//! names the version, the system, the theme in use and the place and message of the panic, with
//! a backtrace.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The crash files kept; older ones are removed when a new one is written.
const KEPT: usize = 20;

/// The theme in use, for the crash file.
static THEME: Mutex<String> = Mutex::new(String::new());

/// Notes `id` as the theme in use, so a crash file names it.
pub fn set_theme(id: &str) {
    if let Ok(mut theme) = THEME.lock()
        && theme.as_str() != id
    {
        theme.clear();
        theme.push_str(id);
    }
}

/// Installs the hook that writes a crash file into `folder` (created when needed), after the
/// default hook has printed the panic.
pub fn install(folder: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let theme = THEME.lock().map(|t| t.clone()).unwrap_or_default();
        let text = report(&message, &location, &theme, &std::backtrace::Backtrace::force_capture().to_string());
        if let Err(error) = write(&folder, &text) {
            eprintln!("could not write the crash file: {error}");
        }
    }));
}

/// The crash file's text.
fn report(message: &str, location: &str, theme: &str, backtrace: &str) -> String {
    format!(
        "Suhina {} crashed\nsystem: {} {}\ntheme: {theme}\nat: {location}\nmessage: {message}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Writes `text` as a new crash file in `folder` and keeps the newest [`KEPT`].
fn write(folder: &Path, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut path = folder.join(format!("crash-{seconds}.txt"));
    let mut n = 1;
    while path.exists() && n < 100 {
        path = folder.join(format!("crash-{seconds}-{n}.txt"));
        n += 1;
    }
    std::fs::write(&path, text)?;
    let mut files: Vec<_> = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    if files.len() > KEPT {
        files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        for old in &files[..files.len() - KEPT] {
            let _removed = std::fs::remove_file(old);
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_file_names_the_place_the_message_and_the_theme_and_old_ones_go() {
        let folder = std::env::temp_dir().join(format!("suhina-crashes-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&folder);
        let text = report("min > max", "src/kit/mixer_controls.rs:58:5", "fl-benchmark", "0: main");
        assert!(text.contains("min > max") && text.contains("mixer_controls.rs:58") && text.contains("fl-benchmark"));
        for _ in 0..KEPT + 3 {
            write(&folder, &text).unwrap();
        }
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), KEPT);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
