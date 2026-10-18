// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The fast paths of a scan: VST3 bundles that describe themselves in `moduleinfo.json` are read
//! without loading anything, and the rest go to a few long-lived `suhina-plugin-host
//! --scan-worker` processes that each read file after file. A worker that crashes or hangs on a
//! file is ended, the file is marked failed with the reason, and a fresh worker takes the rest.
//!
//! Starting a process per file cost about a second each on a busy Windows machine (antivirus
//! checks every start), more than most plug-ins take to load; long-lived workers pay it once.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::scan::{PluginInfo, SCAN_MARKER, Scanned, apply_compatibility, hung_message, parse_scan};

/// How long a file a shell may take: one file that carries many plug-ins (a vendor's shell
/// file) loads each of them, which takes minutes in any host.
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(180);
/// Longest line a worker's answer may be.
const MAX_LINE: u64 = 4 * 1024 * 1024;

/// Whether `path` looks like a shell, which needs [`SHELL_TIMEOUT`].
pub fn is_shell(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().contains("shell"))
}

/// How many workers to run: half the processor's threads, between 1 and 4.
pub fn default_workers() -> usize {
    thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(1, 4)
}

/// What a VST3 bundle's `moduleinfo.json` says it holds, if it has one that lists audio classes.
/// The file is JSON5 (trailing commas); one that cannot be read gives `None`, so the bundle is
/// loaded instead.
pub fn module_info(bundle: &Path) -> Option<Vec<PluginInfo>> {
    if !bundle.is_dir() {
        return None;
    }
    let contents = bundle.join("Contents");
    let file = [contents.join("Resources").join("moduleinfo.json"), contents.join("moduleinfo.json")]
        .into_iter()
        .find(|f| f.is_file())?;
    let text = std::fs::read_to_string(file).ok()?;
    let value: Value = json5::from_str(&text).ok()?;
    let factory_vendor = value.pointer("/Factory Info/Vendor").and_then(Value::as_str).unwrap_or_default();
    let text_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let mut plugins: Vec<PluginInfo> = value
        .get("Classes")?
        .as_array()?
        .iter()
        .filter(|c| c.get("Category").and_then(Value::as_str) == Some("Audio Module Class"))
        .filter_map(|c| {
            let id = text_of(c, "CID").to_ascii_uppercase();
            if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let vendor =
                Some(text_of(c, "Vendor")).filter(|v| !v.is_empty()).unwrap_or_else(|| factory_vendor.to_string());
            Some(PluginInfo {
                path: bundle.to_path_buf(),
                id,
                name: text_of(c, "Name"),
                vendor,
                version: text_of(c, "Version"),
                features: c
                    .get("Sub Categories")
                    .and_then(Value::as_array)
                    .map(|s| s.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default(),
                replaces: Vec::new(),
            })
        })
        .collect();
    if let Some(list) = value.get("Compatibility") {
        apply_compatibility(&mut plugins, list);
    }
    if plugins.is_empty() { None } else { Some(plugins) }
}

/// A running worker: its process, its input, and the lines it prints.
struct Worker {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
}

impl Worker {
    fn start(host: &Path) -> Result<Self, String> {
        let mut child = crate::host_command(host)
            .arg("--scan-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start the scanner: {e}"))?;
        let input = child.stdin.take().ok_or("the scanner has no input")?;
        let output = child.stdout.take().ok_or("the scanner has no output")?;
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut line = String::new();
            loop {
                line.clear();
                match (&mut reader).take(MAX_LINE).read_line(&mut line) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        if sender.send(line.trim_end().to_string()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(Self { child, input, lines })
    }

    /// Asks for `file` and waits up to `timeout` for the answer. `Err` means the worker is
    /// gone (crashed, hung and ended); the text says why.
    fn read(&mut self, file: &str, timeout: Duration) -> Result<Scanned, String> {
        if writeln!(self.input, "{file}").and_then(|()| self.input.flush()).is_err() {
            return Err(self.ended("crashed while loading"));
        }
        let deadline = Instant::now() + timeout;
        loop {
            match self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => {
                    let Some(answer) = line.strip_prefix(SCAN_MARKER) else { continue };
                    let Ok(value) = serde_json::from_str::<Value>(answer) else { continue };
                    if value.get("file").and_then(Value::as_str) == Some(file) {
                        return Ok(parse_scan(&value, file));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err(hung_message(timeout));
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.ended("crashed while loading")),
            }
        }
    }

    /// Why the worker stopped: `what`, with its exit status when there is one.
    fn ended(&mut self, what: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return format!("{what} ({status})");
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        what.to_string()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing its input tells a worker to finish; one that does not is ended.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads `files` (paths, or `test:` names) with `workers` long-lived scanner processes, each
/// file within `timeout(file)`. Returns one result per file, in order. Shells go first, so the
/// slowest files overlap with the rest.
pub fn scan_files(
    host: &Path,
    files: &[String],
    workers: usize,
    timeout: &(dyn Fn(&str) -> Duration + Sync),
) -> Vec<Scanned> {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&i| !files.get(i).is_some_and(|f| is_shell(Path::new(f))));
    let queue = Mutex::new(order.into_iter().collect::<VecDeque<usize>>());
    let results = Mutex::new(vec![None; files.len()]);
    thread::scope(|scope| {
        for _ in 0..workers.clamp(1, files.len().max(1)) {
            scope.spawn(|| {
                let mut worker: Option<Worker> = None;
                loop {
                    let Some(index) = queue.lock().ok().and_then(|mut q| q.pop_front()) else { return };
                    let Some(file) = files.get(index) else { continue };
                    let scanned = if file.contains('\n') {
                        Scanned::Failed("the path has a line break".into())
                    } else {
                        if worker.is_none() {
                            worker = Worker::start(host).ok();
                        }
                        match worker.as_mut().map(|w| w.read(file, timeout(file))) {
                            Some(Ok(scanned)) => scanned,
                            Some(Err(why)) => {
                                worker = None;
                                Scanned::Failed(why)
                            }
                            None => Scanned::Failed("could not start the scanner".into()),
                        }
                    };
                    if let Ok(mut results) = results.lock()
                        && let Some(slot) = results.get_mut(index)
                    {
                        *slot = Some(scanned);
                    }
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Scanned::Failed("not scanned".into())))
        .collect()
}

/// [`scan_files`], then the files that failed read once more, one at a time, by a single
/// worker: a failure can be passing (a plug-in that waits for a lock another process of its
/// vendor holds gives no factory or hangs), and side-by-side loads make that more likely. The
/// second result is the one kept. Also returns the indices of the files read twice.
pub fn scan_files_retrying(
    host: &Path,
    files: &[String],
    workers: usize,
    timeout: &(dyn Fn(&str) -> Duration + Sync),
) -> (Vec<Scanned>, Vec<usize>) {
    let mut results = scan_files(host, files, workers, timeout);
    let failed: Vec<usize> =
        results.iter().enumerate().filter(|(_, r)| matches!(r, Scanned::Failed(_))).map(|(i, _)| i).collect();
    if failed.is_empty() {
        return (results, failed);
    }
    let again: Vec<String> = failed.iter().filter_map(|&i| files.get(i).cloned()).collect();
    for (&index, scanned) in failed.iter().zip(scan_files(host, &again, 1, timeout)) {
        if let Some(slot) = results.get_mut(index) {
            *slot = scanned;
        }
    }
    (results, failed)
}

/// `normal`, or [`SHELL_TIMEOUT`] for a shell.
pub fn timeout_for(path: &str, normal: Duration) -> Duration {
    if is_shell(Path::new(path)) { SHELL_TIMEOUT } else { normal }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_are_recognised() {
        assert!(is_shell(Path::new("C:/VST3/WaveShell1-VST3 16.7_x64.vst3")));
        assert!(is_shell(Path::new("/vst3/PreSonus VST3 Shell.vst3")));
        assert!(!is_shell(Path::new("/vst3/FabFilter Pro-R 2.vst3")));
        assert!((1..=4).contains(&default_workers()));
    }

    #[test]
    fn module_info_is_read_from_a_bundle() {
        let dir = std::env::temp_dir().join(format!("suhina-moduleinfo-{}", std::process::id()));
        let bundle = dir.join("Thing.vst3");
        let resources = bundle.join("Contents").join("Resources");
        let _ = std::fs::create_dir_all(&resources);
        let info = r#"{
          "Name": "Thing", "Version": "1.0.0",
          "Factory Info": { "Vendor": "Maker", "Flags": { "Unicode": true, }, },
          "Classes": [
            { "CID": "abcdef019182faeb49746d78426e6477", "Category": "Audio Module Class", "Name": "Thing",
              "Version": "1.2.3", "Sub Categories": [ "Fx", "Delay", ], },
            { "CID": "ABCDEF011234ABCD49746D78426E6477", "Category": "Component Controller Class", "Name": "Thing", },
            { "CID": "not hex", "Category": "Audio Module Class", "Name": "Broken", },
          ],
        }"#;
        let _ = std::fs::write(resources.join("moduleinfo.json"), info);
        let plugins = module_info(&bundle).unwrap_or_default();
        let [thing] = plugins.as_slice() else { panic!("{plugins:?}") };
        assert_eq!(thing.id, "ABCDEF019182FAEB49746D78426E6477");
        assert_eq!((thing.name.as_str(), thing.vendor.as_str(), thing.version.as_str()), ("Thing", "Maker", "1.2.3"));
        assert_eq!(thing.features, ["Fx", "Delay"]);
        assert_eq!(thing.path, bundle);

        let _ = std::fs::write(resources.join("moduleinfo.json"), "{ broken");
        assert!(module_info(&bundle).is_none());
        let _ = std::fs::write(resources.join("moduleinfo.json"), r#"{ "Classes": [] }"#);
        assert!(module_info(&bundle).is_none());
        assert!(module_info(&dir.join("missing.vst3")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
