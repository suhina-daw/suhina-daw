// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-plugin-host`: the process plug-ins run in, never the engine's (ADR 0006).
//!
//! `suhina-plugin-host --control <address>` connects to the engine's control socket and
//! answers its messages (`suhina_plugins::protocol`) on the main thread, which also serves the
//! plug-ins' main-thread callbacks and their editors. It runs any number of plug-in instances,
//! each loaded and unloaded on its own and named by the number the engine gave it; each has
//! its own link and a thread of its own that serves its audio blocks through `suhina-ipc`.
//! A block may ask for other instances to run after the link's own; the thread that has it runs
//! them in turn and answers once ([`audio`]).
//! When the control connection closes, the engine is gone and the process exits.
//!
//! What it can load: CLAP plug-ins ([`clap`]), the CLAP test plug-ins built in for tests
//! ([`test_clap`], [`test_clap_delay`], [`test_clap_ports`]), and simple test processors
//! ([`processors`]) that crash or hang on purpose. With the `vst2` feature (ADR 0022) it
//! gains the VST2 interface layer (`vst2`, under the feature, so no link here): the ported VeSTige definitions, the mock
//! plug-in `test:vst2` and the `--vst2-probe` tool.
//!
//! A plug-in may report another latency while it runs. Between messages the main thread asks
//! each instance whether it did (restarting its processing first where the format asks for
//! that) and tells the engine unasked (`latency_changed`).

mod ara;
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unsafe_op_in_unsafe_fn,
    clippy::all,
    clippy::pedantic
)]
// The ARA headers pack structs to 1 byte on x86 and to 8 on ARM: each CPU has bindings of its own.
#[cfg_attr(target_arch = "aarch64", path = "ara_sys/arm.rs")]
#[cfg_attr(not(target_arch = "aarch64"), path = "ara_sys/x86.rs")]
mod ara_sys;
mod audio;
mod clap;
mod param_notes;
mod processors;
mod run_loop;
mod test_clap;
mod test_clap_delay;
mod test_clap_ports;
mod test_vst3;
#[cfg(feature = "vst2")]
mod vst2;
mod vst3;
mod vst3_editor;
mod vst3_events;
mod vst3_message;
mod vst3_params;
mod window;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::{Duration, Instant};

use interprocess::TryClone;
use processors::Processor;
use suhina_ipc::Served;
use suhina_plugins::ParamInfo;
use suhina_plugins::protocol::{Message, VERSION};

/// How long loading waits for a new instance's audio thread to serve, so that blocks naming it
/// find it.
const SERVING_WAIT: Duration = Duration::from_secs(2);
/// How often the main thread looks for plug-in callbacks while no message comes.
const TICK: Duration = Duration::from_millis(10);
/// The same while an editor is open where the main thread cannot wait for its window's events
/// (X11): often enough for input and drawing to feel immediate.
#[cfg(not(windows))]
const EDITOR_TICK: Duration = Duration::from_millis(1);
/// How long unloading waits for an instance's audio thread to stop; one still running then is
/// stuck in the plug-in, and the instance is left behind rather than taking the process with it.
pub(crate) const UNLOAD_WAIT: Duration = Duration::from_secs(1);
/// How long a VST3 plug-in has to settle after it loads before it is released: one let go of
/// sooner (loaded again at once as ARA, say) is kept, stopped and silent, until then.
const SETTLE: Duration = Duration::from_secs(5);

thread_local! {
    /// VST3 plug-ins unloaded, stopped and out of their document, and when each is released
    /// with its editor. Released from the main loop ([`release_settled`]) after the engine has
    /// its answer, so an editor embedded in the engine's window is taken down while the engine
    /// answers that window's messages. Some plug-ins also finish starting up through the main
    /// thread's messages after they load, and releasing one before that waits for it on this
    /// very thread: Melodyne, let go of a tenth of a second after it loaded, held the host until
    /// the engine gave up on it; one is kept until [`SETTLE`] after it loaded.
    static SETTLING: std::cell::RefCell<Vec<(Instant, vst3::Vst3Plugin)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Releases the plug-ins in [`SETTLING`] whose time has come.
fn release_settled() {
    let due: Vec<vst3::Vst3Plugin> = SETTLING.with(|settling| {
        let Ok(mut settling) = settling.try_borrow_mut() else { return Vec::new() };
        let now = Instant::now();
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut *settling).into_iter().partition(|(at, _)| *at <= now);
        *settling = keep;
        due.into_iter().map(|(_, plugin)| plugin).collect()
    });
    for plugin in due {
        suhina_plugins::trace!("vst3: releasing an unloaded plug-in and its editor");
        drop(plugin);
    }
}
/// How often the process reports its RAM to the engine's control bar.
const MEMORY_EVERY: Duration = Duration::from_secs(1);
/// Most instances one process runs.
const MAX_INSTANCES: usize = 1024;

/// Gives the calling thread, an instance's audio thread, real-time priority for as long as the
/// value returned lives, so it answers the engine's blocks as soon as they come: the engine
/// waits only until each block's deadline (ADR 0006). MMCSS "Pro Audio" on Windows, `SCHED_FIFO`
/// or rtkit on Linux, as far as the system allows ([`suhina_realtime`]); where it allows nothing
/// the thread runs at its normal priority. The user hears of a refusal from the engine, whose
/// render thread asks the same system first.
pub(crate) fn real_time_thread() -> suhina_realtime::RealTime {
    suhina_realtime::RealTime::join(1024, 48_000)
}

/// Asks an audio thread to stop and waits up to `wait` for it to end; whether it did (or there
/// was none).
pub(crate) fn audio_stopped<T>(stop: &AtomicBool, audio: Option<&thread::JoinHandle<T>>, wait: Duration) -> bool {
    stop.store(true, Ordering::Release);
    let deadline = Instant::now() + wait;
    loop {
        if audio.is_none_or(thread::JoinHandle::is_finished) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

/// Sets the main thread up as a plug-in's editor expects a host's window thread to be: on
/// Windows with OLE (and so COM, apartment threaded) initialised: plug-in editors use OLE for
/// drag and drop, the clipboard and text services, and some fail to open when the thread has not
/// called `OleInitialize`.
fn prepare_main_thread() {
    #[cfg(windows)]
    {
        // SAFETY: called once, on the main thread, before any window or plug-in; a null
        // reserved argument as the function requires.
        let _ = unsafe { windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null_mut()) };
    }
}

fn main() {
    prepare_main_thread();
    let args: Vec<String> = std::env::args().collect();
    let address = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("--control"), Some(address)) => address,
        (Some("--scan"), Some(file)) => scan(file),
        (Some("--scan-worker"), None) => scan_worker(),
        #[cfg(feature = "vst2")]
        (Some("--vst2-probe"), Some(name)) => vst2_probe(name),
        _ => {
            #[cfg(feature = "vst2")]
            eprintln!(
                "usage: suhina-plugin-host --control <address> | --scan <file> | --scan-worker | \
                 --vst2-probe <plug-in>"
            );
            #[cfg(not(feature = "vst2"))]
            eprintln!("usage: suhina-plugin-host --control <address> | --scan <file> | --scan-worker");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(address) {
        eprintln!("suhina-plugin-host: {e}");
        std::process::exit(1);
    }
}

/// `--vst2-probe <plug-in>`: prints the report of the probe over the VST2 plug-in the name
/// gives (a VST2 file, or `test:vst2` for the built-in mock) and exits. A tool of the `vst2`
/// feature (ADR 0022): what the probe learns settles the interface's *assumed* facts, and
/// this process is disposable, as a scanner's is.
#[cfg(feature = "vst2")]
fn vst2_probe(name: &str) -> ! {
    match vst2::probe::run_tool(name) {
        Ok(report) => {
            println!("{report}");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("suhina-plugin-host: {e}");
            std::process::exit(1);
        }
    }
}

/// `--scan <file>`: prints the plug-ins the file holds on one line after
/// [`suhina_plugins::scan::SCAN_MARKER`] and exits. A plug-in that crashes or hangs while
/// loading takes this process with it, and the scanner in the engine notices.
///
/// For tests, `test:clap` describes the built-in test plug-in, `test:crash` exits without an
/// answer, `test:hang` never answers and, in debug builds, `test:flaky:<file>` fails the first time (while
/// `<file>` does not exist; it then makes it) and describes the test plug-in after that.
fn scan(file: &str) -> ! {
    use suhina_plugins::scan::{SCAN_MARKER, scan_error_json, scan_json};
    let result = describe(file);
    let (line, code) = match result {
        Ok(plugins) => (scan_json(&plugins), 0),
        Err(message) => (scan_error_json(&message), 1),
    };
    // A plug-in may have printed to stdout while loading; the marker sets this line apart.
    println!("{SCAN_MARKER}{line}");
    std::process::exit(code);
}

/// The plug-ins `file` holds (see [`scan`] for the `test:` names).
fn describe(file: &str) -> Result<Vec<suhina_plugins::scan::PluginInfo>, String> {
    match file {
        "test:crash" => std::process::exit(3),
        "test:hang" => loop {
            thread::sleep(Duration::from_secs(60));
        },
        "test:clap" | "test:clap-mix" | "test:clap-delay" | "test:clap-ports" => clap::describe(file),
        "test:vst3" | "test:vst3-mix" => vst3::describe(file),
        #[cfg(feature = "vst2")]
        "test:vst2" => vst2::host::describe(file),
        #[cfg(debug_assertions)]
        _ if file.starts_with(FLAKY) => flaky(file),
        // A VST2 plug-in: a `.vst` bundle, a `.dll` or a `.so`, behind the `vst2` feature
        // (ADR 0022); a build without it lists the file as failed.
        _ if suhina_plugins::scan::is_vst2(std::path::Path::new(file)) => {
            #[cfg(feature = "vst2")]
            {
                vst2::host::describe(file)
            }
            #[cfg(not(feature = "vst2"))]
            Err("VST2 plug-ins are not hosted in this build (ADR 0022)".into())
        }
        _ if file.to_ascii_lowercase().ends_with(".vst3") => vst3::describe(&format!("vst3:{file}")),
        _ => clap::describe(&format!("clap:{file}")),
    }
}

/// The `test:` name of a file that fails to load once ([`flaky`]). Debug builds only: it writes
/// the file it is given, which a release host has no use for.
#[cfg(debug_assertions)]
const FLAKY: &str = "test:flaky:";

/// `test:flaky:<file>`: a plug-in whose first load fails as a passing failure does (no factory)
/// and whose later ones work. Whether it was loaded before is `<file>`'s existence, so it holds
/// across scan processes.
#[cfg(debug_assertions)]
fn flaky(file: &str) -> Result<Vec<suhina_plugins::scan::PluginInfo>, String> {
    let marker = std::path::Path::new(file.strip_prefix(FLAKY).unwrap_or_default());
    if marker.exists() {
        return clap::describe("test:clap");
    }
    std::fs::write(marker, b"loaded once").map_err(|e| format!("could not mark the first load: {e}"))?;
    Err("the plug-in has no factory".into())
}

/// `--scan-worker`: reads one file name per line from standard input and prints each answer on
/// a line after [`suhina_plugins::scan::SCAN_MARKER`], until the input ends. The engine's
/// scanner keeps a few of these running so a scan does not start a process per file; one that
/// crashes or hangs on a file is ended and replaced.
fn scan_worker() -> ! {
    use std::io::BufRead;
    use suhina_plugins::scan::{SCAN_MARKER, scan_answer};
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(file) = line else { break };
        let file = file.trim_end();
        let answer = match describe(file) {
            Ok(plugins) => scan_answer(file, Ok(&plugins)),
            Err(message) => scan_answer(file, Err(&message)),
        };
        println!("{SCAN_MARKER}{answer}");
        let _ = std::io::stdout().flush();
    }
    std::process::exit(0);
}

/// What is loaded.
enum Loaded {
    Clap(clap::ClapPlugin),
    Vst3(vst3::Vst3Plugin),
    #[cfg(feature = "vst2")]
    Vst2(vst2::host::Vst2Plugin),
    /// `reported` is the latency the engine was last told.
    Test {
        processor: Arc<dyn Processor>,
        stop: Arc<AtomicBool>,
        audio: thread::JoinHandle<()>,
        reported: u32,
    },
}

impl Loaded {
    fn latency(&self) -> u32 {
        match self {
            Self::Clap(plugin) => plugin.latency(),
            Self::Vst3(plugin) => plugin.latency(),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.latency(),
            Self::Test { processor, .. } => processor.latency(),
        }
    }

    /// A latency the plug-in reported since this was last asked, after restarting its
    /// processing where its format asks for that: the engine is to be told.
    /// The notes an ARA plug-in found in its regions since the last call (#720).
    fn take_ara_analysis(&self) -> Option<Option<f32>> {
        match self {
            Self::Vst3(plugin) => plugin.take_ara_analysis(),
            Self::Clap(_) | Self::Test { .. } => None,
            #[cfg(feature = "vst2")]
            Self::Vst2(_) => None,
        }
    }

    fn take_ara_notes(&self) -> Vec<(String, Vec<suhina_plugins::protocol::AraNote>, bool)> {
        match self {
            Self::Vst3(plugin) => plugin.take_ara_notes(),
            Self::Clap(_) | Self::Test { .. } => Vec::new(),
            #[cfg(feature = "vst2")]
            Self::Vst2(_) => Vec::new(),
        }
    }

    fn latency_changed(&mut self) -> Option<u32> {
        match self {
            Self::Clap(plugin) => plugin.latency_changed(),
            Self::Vst3(plugin) => plugin.latency_changed(),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.latency_changed(),
            Self::Test { processor, reported, .. } => {
                let now = processor.latency();
                (now != *reported).then(|| {
                    *reported = now;
                    now
                })
            }
        }
    }

    fn open_editor(&mut self, mode: window::Mode) -> Result<window::Size, String> {
        match self {
            Self::Clap(plugin) => plugin.open_editor(mode),
            Self::Vst3(plugin) => plugin.open_editor(mode),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.open_editor(mode),
            Self::Test { .. } => Err("the plug-in has no editor".into()),
        }
    }

    /// Puts an embedded editor where the engine says.
    fn place_editor(&self, placement: window::Placement) {
        match self {
            Self::Clap(plugin) => plugin.place_editor(placement),
            Self::Vst3(plugin) => plugin.place_editor(placement),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.place_editor(placement),
            Self::Test { .. } => {}
        }
    }

    /// Brings a plug-in's ARA document in line with the track's clips.
    fn ara_update(&mut self, document: &suhina_plugins::protocol::AraDocument) -> Result<(), String> {
        match self {
            Self::Vst3(plugin) => plugin.ara_update(document),
            _ => Err("only a VST3 plug-in can be bound to an ARA document".into()),
        }
    }

    /// The edits of one modification and its source, stored on their own (ADR 0028 step 6).
    fn ara_export(&self, modification: &str, source: &str) -> Result<(String, Vec<u8>), String> {
        match self {
            Self::Vst3(plugin) => plugin.ara_export(modification, source),
            _ => Err("only a VST3 plug-in can be bound to an ARA document".into()),
        }
    }

    /// Gives the plug-in edits another track's stored, for one of its modifications.
    fn ara_import(&self, id: &str, bytes: &[u8], from: &str, to: &str, source: &str) -> Result<(), String> {
        match self {
            Self::Vst3(plugin) => plugin.ara_import(id, bytes, from, to, source),
            _ => Err("only a VST3 plug-in can be bound to an ARA document".into()),
        }
    }

    /// Whether the plug-in supports ARA.
    fn edits_clips(&self) -> bool {
        match self {
            Self::Vst3(plugin) => plugin.edits_clips(),
            _ => false,
        }
    }

    /// Tells a plug-in bound to an ARA document which region the engine has selected.
    fn ara_select(&self, region: Option<&str>) {
        if let Self::Vst3(plugin) = self {
            plugin.ara_select(region);
        }
    }

    /// A size the open editor got since this was last asked, to tell the engine.
    fn editor_resized(&mut self) -> Option<window::Size> {
        match self {
            Self::Clap(plugin) => plugin.editor_resized(),
            Self::Vst3(plugin) => plugin.editor_resized(),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.editor_resized(),
            Self::Test { .. } => None,
        }
    }

    fn close_editor(&mut self) {
        match self {
            Self::Clap(plugin) => plugin.close_editor(),
            Self::Vst3(plugin) => plugin.close_editor(),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.close_editor(),
            Self::Test { .. } => {}
        }
    }

    /// Keeps an open editor in step with its window; whether the user closed it.
    fn editor_tick(&mut self) -> bool {
        match self {
            Self::Clap(plugin) => plugin.editor_tick(),
            Self::Vst3(plugin) => plugin.editor_tick(),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.editor_tick(),
            Self::Test { .. } => false,
        }
    }

    fn params(&mut self) -> Result<Vec<ParamInfo>, String> {
        match self {
            Self::Clap(plugin) => Ok(plugin.params()),
            Self::Vst3(plugin) => Ok(plugin.params()),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.params(),
            Self::Test { .. } => Ok(Vec::new()),
        }
    }

    fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        match self {
            Self::Clap(plugin) => plugin.set_param(id, value),
            Self::Vst3(plugin) => plugin.set_param(id, value),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.set_param(id, value),
            Self::Test { .. } => Err(format!("no parameter {id}")),
        }
    }

    /// The component's state and, for VST3, the controller's.
    fn state(&mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
        match self {
            Self::Clap(plugin) => Ok((plugin.state()?, None)),
            Self::Vst3(plugin) => Ok((plugin.state()?, plugin.controller_state())),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => Ok((plugin.state()?, None)),
            Self::Test { processor, .. } => Ok((processor.state(), None)),
        }
    }

    fn set_state(&mut self, data: &[u8], controller: Option<&[u8]>) -> Result<(), String> {
        match self {
            Self::Clap(plugin) => plugin.set_state(data),
            Self::Vst3(plugin) => plugin.set_state(data, controller),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.set_state(data),
            Self::Test { processor, .. } => processor.set_state(data),
        }
    }

    fn tick(&mut self) -> Vec<(u32, f64)> {
        match self {
            // What a CLAP plug-in changed itself comes out with the blocks as output parameter
            // events, which its audio thread hands over here; its listing still read its values
            // again as they stand.
            Self::Clap(plugin) => plugin.tick(),
            Self::Vst3(plugin) => plugin.tick(),
            #[cfg(feature = "vst2")]
            Self::Vst2(plugin) => plugin.tick(),
            Self::Test { .. } => Vec::new(),
        }
    }

    /// Closes the editor, stops the audio thread and lets go of the plug-in. An audio thread
    /// that does not stop is stuck in the plug-in's code: the plug-in is then left as it is
    /// (never deactivated or freed under it), so the other instances go on.
    fn unload(self) {
        match self {
            Self::Clap(mut plugin) => {
                plugin.close_editor();
                if plugin.stop_audio(UNLOAD_WAIT) {
                    drop(plugin);
                } else {
                    std::mem::forget(plugin);
                }
            }
            Self::Vst3(mut plugin) => {
                suhina_plugins::trace!("vst3 unload: stop audio");
                if plugin.stop_audio(UNLOAD_WAIT) {
                    // The editor closes and the plug-in goes from the main loop, after the engine
                    // has its answer: an editor embedded in the engine's window is a child of a
                    // window of the engine's process, and taking it down sends that window
                    // messages the engine answers only once it stops waiting for this unload
                    // (Melodyne held both for as long as the engine waited, #816). One that has
                    // not settled since it loaded waits for that too.
                    plugin.leave_document();
                    let at = Instant::now() + SETTLE.saturating_sub(plugin.age());
                    suhina_plugins::trace!("vst3 unload: audio stopped; editor closed and released from the main loop");
                    SETTLING.with(|settling| settling.borrow_mut().push((at, plugin)));
                } else {
                    plugin.close_editor();
                    std::mem::forget(plugin);
                }
            }
            Self::Test { stop, audio, .. } => {
                if audio_stopped(&stop, Some(&audio), UNLOAD_WAIT) {
                    let _ = audio.join();
                }
            }
            #[cfg(feature = "vst2")]
            Self::Vst2(mut plugin) => {
                suhina_plugins::trace!("vst2 unload: stop audio");
                if plugin.stop_audio(UNLOAD_WAIT) {
                    // The audio thread's exit suspended the plug-in; the editor closes next
                    // (an embedded one is a child of the engine's window, whose messages the
                    // engine answers only once it stops waiting), and then the plug-in goes.
                    plugin.close_editor();
                    plugin.close();
                    drop(plugin);
                } else {
                    // Stuck in the plug-in: left as it is, never closed or freed under it.
                    std::mem::forget(plugin);
                }
            }
        }
    }
}

fn load(
    instance: u32,
    plugin: &str,
    link: &str,
    sample_rate: f64,
    ara: Option<suhina_plugins::protocol::AraDocument>,
) -> Result<Loaded, String> {
    let loaded = start(instance, plugin, link, sample_rate, ara)?;
    // Its thread starts serving at once; a plug-in whose processing would not start never does.
    audio::wait_serving(instance, SERVING_WAIT);
    Ok(loaded)
}

fn start(
    instance: u32,
    plugin: &str,
    link: &str,
    sample_rate: f64,
    ara: Option<suhina_plugins::protocol::AraDocument>,
) -> Result<Loaded, String> {
    let served = Served::open(link).map_err(|e| e.to_string())?;
    if plugin.starts_with("clap:")
        || plugin == "test:clap"
        || plugin == "test:clap-mix"
        || plugin == "test:clap-delay"
        || plugin == "test:clap-ports"
    {
        return clap::ClapPlugin::load(plugin, instance, served, sample_rate).map(Loaded::Clap);
    }
    if plugin.starts_with("vst3:") || plugin == "test:vst3" || plugin == "test:vst3-mix" {
        return vst3::Vst3Plugin::load(plugin, instance, served, sample_rate, ara).map(Loaded::Vst3);
    }
    #[cfg(feature = "vst2")]
    if plugin.starts_with("vst2:") || plugin == "test:vst2" {
        if ara.is_some() {
            return Err("only a VST3 plug-in can be bound to an ARA document".into());
        }
        return vst2::host::Vst2Plugin::load(plugin, instance, served, sample_rate).map(Loaded::Vst2);
    }
    if ara.is_some() {
        return Err("only a VST3 plug-in can be bound to an ARA document".into());
    }
    let processor = processors::create(plugin)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (block, audio_stop, mut served) = (processors::TestBlock(processor.clone()), stop.clone(), served);
    let audio = thread::Builder::new()
        .name("plug-in audio".into())
        .spawn(move || {
            let _realtime = real_time_thread();
            audio::serve(instance, block, &mut served, &audio_stop);
        })
        .map_err(|e| e.to_string())?;
    let reported = processor.latency();
    Ok(Loaded::Test { processor, stop, audio, reported })
}

/// Writes one message to the engine.
/// The engine's end of the connection, shared: the main loop and a plug-in's transport request,
/// sent the moment it is made, each write whole lines to it, one at a time.
struct Shared<W>(Arc<std::sync::Mutex<W>>);

impl<W> Clone for Shared<W> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<W: Write> Write for Shared<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).write_all(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).flush()
    }
}

/// Where [`send_now`] writes: the connection, once the host is connected.
static NOW: std::sync::Mutex<Option<Box<dyn Write + Send>>> = std::sync::Mutex::new(None);

/// Sends `message` at once, from whichever thread, rather than at the main loop's next turn:
/// an ARA plug-in's transport request, which waited for its own click to be handled before
/// (100 ms or more, so its playhead went to the new place late). Returns whether it was sent.
pub(crate) fn send_now(message: &Message) -> bool {
    let mut now = NOW.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    now.as_mut().is_some_and(|writer| send(writer, message).is_ok())
}

/// A message as the trace writes it: a list of numbers (a plug-in's state, an archive) as its
/// length, and cut to `most` characters.
fn traced(message: &Message, most: usize) -> String {
    // A state or an archive can be megabytes: formatted whole it took longer than the read.
    let bytes = |data: &[u8]| format!("<{} bytes>", data.len());
    let text = match message {
        Message::State { instance, data, controller } | Message::SetState { instance, data, controller } => {
            let kind = if matches!(message, Message::State { .. }) { "State" } else { "SetState" };
            format!(
                "{kind} {{ instance: {instance}, data: {}, controller: {:?} }}",
                bytes(data),
                controller.as_deref().map(bytes)
            )
        }
        Message::AraExported { instance, archive_id, data } => {
            format!("AraExported {{ instance: {instance}, archive_id: {archive_id:?}, data: {} }}", bytes(data))
        }
        Message::AraImport { instance, archive_id, data, from, to, source } => format!(
            "AraImport {{ instance: {instance}, archive_id: {archive_id:?}, data: {}, from: {from:?}, to: {to:?}, source: {source:?} }}",
            bytes(data)
        ),
        _ => format!("{message:?}"),
    };
    let mut out = String::with_capacity(text.len().min(most + 16));
    let mut rest = text.as_str();
    while let Some(open) = rest.find('[') {
        let (before, list) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = list.find(']') else {
            rest = list;
            break;
        };
        let inside = &list[1..close];
        if inside.len() > 24 && inside.bytes().all(|b| b.is_ascii_digit() || b == b',' || b == b' ' || b == b'-') {
            out.push_str(&format!("<{} numbers>", inside.split(',').count()));
        } else {
            out.push_str(&list[..=close]);
        }
        rest = &list[close + 1..];
    }
    out.push_str(rest);
    let cut = out.char_indices().nth(most).map_or(out.len(), |(i, _)| i);
    out.truncate(cut);
    out
}

/// Whether a memory report goes into the trace: one a minute, or when it moved by a tenth since
/// the last one traced. Every other message does.
fn memory_worth_tracing(message: &Message) -> bool {
    static LAST: std::sync::Mutex<Option<(u64, Instant)>> = std::sync::Mutex::new(None);
    let Message::Memory { bytes } = message else { return true };
    let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let due = match *last {
        Some((was, at)) => at.elapsed() >= Duration::from_secs(60) || bytes.abs_diff(was) > was / 10,
        None => true,
    };
    if due {
        *last = Some((*bytes, Instant::now()));
    }
    due
}

fn send(writer: &mut impl Write, message: &Message) -> std::io::Result<()> {
    if suhina_plugins::trace::on() && !matches!(message, Message::AraNotes { .. }) && memory_worth_tracing(message) {
        suhina_plugins::trace!("host sends {}", traced(message, 300));
    }
    let mut out = message.encode();
    out.push('\n');
    writer.write_all(out.as_bytes())?;
    writer.flush()
}

/// The engine's messages, on Windows passed through a thread that wakes the main thread when
/// one comes, so the main thread can wait for its windows' messages instead of polling them.
#[cfg(windows)]
fn wakeful(messages: std::sync::mpsc::Receiver<Message>) -> std::sync::mpsc::Receiver<Message> {
    let waker = window::Waker::current();
    let (sender, receiver) = std::sync::mpsc::channel();
    let _forwarder = thread::Builder::new().name("control messages".into()).spawn(move || {
        for message in messages {
            if sender.send(message).is_err() {
                return;
            }
            waker.wake();
        }
        // The engine is gone: wake the main thread to notice.
        waker.wake();
    });
    receiver
}

#[cfg(not(windows))]
fn wakeful(messages: std::sync::mpsc::Receiver<Message>) -> std::sync::mpsc::Receiver<Message> {
    messages
}

/// The next message from the engine, or a timeout after the main thread has had something
/// else to do: on Windows a window message (an editor's input, painting, a plug-in's timer),
/// which is then served at once; elsewhere a tick.
#[cfg(windows)]
fn next_message(messages: &std::sync::mpsc::Receiver<Message>) -> Result<Message, RecvTimeoutError> {
    match messages.try_recv() {
        Ok(message) => Ok(message),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            window::wait(TICK);
            messages.try_recv().map_err(|e| match e {
                std::sync::mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                std::sync::mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        }
    }
}

#[cfg(not(windows))]
fn next_message(messages: &std::sync::mpsc::Receiver<Message>) -> Result<Message, RecvTimeoutError> {
    messages.recv_timeout(if window::any_open() { EDITOR_TICK } else { TICK })
}

/// The instances loaded, by the number the engine gave each.
type Instances = BTreeMap<u32, Loaded>;

fn run(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    suhina_plugins::trace!("host up, connecting");
    let stream = suhina_plugins::connect(address)?;
    suhina_plugins::trace!("host connected");
    let mut writer = Shared(Arc::new(std::sync::Mutex::new(stream.try_clone()?)));
    *NOW.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(writer.clone()));
    let messages = wakeful(suhina_plugins::read_lines(stream));
    let mut instances = Instances::new();
    let mut last_memory = Instant::now();
    // Messages read while placements and sizes were gathered, to answer next, in order.
    let mut held: std::collections::VecDeque<Message> = std::collections::VecDeque::new();
    loop {
        // Between messages: the plug-ins' main-thread callbacks and their editors' windows.
        window::pump();
        release_settled();
        // Space in an editor window plays or stops the engine, as it does in Suhina's own.
        for _ in 0..window::take_transport_keys() {
            send(&mut writer, &Message::TransportKey)?;
        }
        // The computer keyboard's note keys pressed in an editor window, while it plays (#944).
        for (key, pressed) in window::take_typed_keys() {
            send(&mut writer, &Message::TypedKey { key, pressed })?;
        }
        // What ARA plug-ins asked of the transport (their play buttons, their timeline).
        for request in ara::take_playback_requests() {
            send(&mut writer, &Message::AraPlayback(request))?;
        }
        // The engine's control bar adds this process's RAM to the program's; report it without
        // being asked, off the control thread, once a second.
        if last_memory.elapsed() >= MEMORY_EVERY {
            if let Some(stats) = memory_stats::memory_stats() {
                send(&mut writer, &Message::Memory { bytes: stats.physical_mem as u64 })?;
            }
            last_memory = Instant::now();
        }
        for (instance, loaded) in &mut instances {
            for (id, value) in loaded.tick() {
                send(&mut writer, &Message::ParamChanged { instance: *instance, id, value })?;
            }
            for (region, notes, truncated) in loaded.take_ara_notes() {
                send(&mut writer, &Message::AraNotes { instance: *instance, region, notes, truncated })?;
            }
            if let Some(progress) = loaded.take_ara_analysis() {
                send(&mut writer, &Message::AraAnalysis { instance: *instance, progress })?;
            }
            if let Some(latency) = loaded.latency_changed() {
                send(&mut writer, &Message::LatencyChanged { instance: *instance, latency })?;
            }
            if loaded.editor_tick() {
                send(&mut writer, &Message::EditorClosed { instance: *instance })?;
            } else if let Some(size) = loaded.editor_resized() {
                send(
                    &mut writer,
                    &Message::EditorResized { instance: *instance, width: size.width, height: size.height },
                )?;
            }
        }
        let message = match held.pop_front().map_or_else(|| next_message(&messages), Ok) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        // Placements and sizes of an editor queued while the host was busy (a splitter dragged:
        // one of each a frame): only the newest of each counts, so the editor goes straight to
        // where and how big the window wants it now rather than laying itself out again for
        // every step on the way, which a heavy editor (Melodyne) took longer to do than a frame.
        let message = match message {
            Message::PlaceEditor { instance, .. } | Message::FitEditor { instance, .. } if held.is_empty() => {
                let (mut place, mut fit) = (None, None);
                let mut keep = |message: Message| match message {
                    Message::PlaceEditor { instance: same, .. } | Message::FitEditor { instance: same, .. }
                        if same != instance =>
                    {
                        Some(message)
                    }
                    Message::PlaceEditor { .. } => {
                        place = Some(message);
                        None
                    }
                    Message::FitEditor { .. } => {
                        fit = Some(message);
                        None
                    }
                    other => Some(other),
                };
                let mut rest = keep(message);
                while rest.is_none() {
                    let Ok(next) = messages.try_recv() else { break };
                    rest = keep(next);
                }
                held.extend(fit);
                held.extend(rest);
                match place {
                    Some(place) => place,
                    None => match held.pop_front() {
                        Some(first) => first,
                        None => continue,
                    },
                }
            }
            other => other,
        };
        if message == Message::Quit {
            break;
        }
        if suhina_plugins::trace::on() {
            suhina_plugins::trace!("host gets {}", traced(&message, 400));
        }
        if let Some(reply) = answer(&mut instances, &writer, message) {
            send(&mut writer, &reply)?;
        }
    }
    for (_, loaded) in std::mem::take(&mut instances) {
        loaded.unload();
    }
    // The process ends: what has not settled is left to it rather than waited for.
    SETTLING.with(|settling| std::mem::forget(settling.take()));
    Ok(())
}

/// The reply to one message from the engine (anything but `quit`); `None` for a message that
/// takes no reply (`place_editor`). `writer` is the connection the unasked messages go out on.
fn answer<W: Write>(instances: &mut Instances, writer: &Shared<W>, message: Message) -> Option<Message> {
    let failed = |instance: u32, message: String| Message::Error { instance: Some(instance), message };
    let missing = |instance: u32| Message::Error { instance: Some(instance), message: "no such instance".into() };
    Some(match message {
        Message::Hello { .. } => Message::Hello { version: VERSION },
        Message::Ping { id } => Message::Pong { id },
        Message::Load { instance, .. } if instances.contains_key(&instance) => {
            failed(instance, format!("instance {instance} is already loaded"))
        }
        Message::Load { instance, .. } if instances.len() >= MAX_INSTANCES => {
            failed(instance, format!("at most {MAX_INSTANCES} plug-ins run in one process"))
        }
        Message::Load { instance, plugin, link, sample_rate, ara } => {
            match load(instance, &plugin, &link, sample_rate, ara) {
                Ok(fresh) => {
                    let (latency, edits_clips) = (fresh.latency(), fresh.edits_clips());
                    instances.insert(instance, fresh);
                    Message::Loaded { instance, latency, edits_clips }
                }
                Err(message) => failed(instance, message),
            }
        }
        Message::Reconfigure { instance, sample_rate } => {
            let result = match instances.get_mut(&instance) {
                Some(Loaded::Clap(plugin)) => plugin.reconfigure(sample_rate),
                Some(Loaded::Vst3(plugin)) => plugin.reconfigure(sample_rate),
                #[cfg(feature = "vst2")]
                Some(Loaded::Vst2(plugin)) => plugin.reconfigure(sample_rate),
                Some(Loaded::Test { .. }) => Err("this test processor cannot reconfigure".into()),
                None => return Some(missing(instance)),
            };
            match result {
                Ok(latency) => Message::Reconfigured { instance, latency },
                Err(why) => failed(instance, why),
            }
        }
        Message::Unload { instance } => {
            if let Some(mut loaded) = instances.remove(&instance) {
                let began = std::time::Instant::now();
                suhina_plugins::trace!("host unloads {instance}");
                // What its audio thread took in but the session has not heard of yet goes out
                // first, so an edit made just before the unload is not lost with the plug-in.
                // A write that fails here is the connection ending; the loop's next send says
                // so.
                let mut writer = writer.clone();
                for (id, value) in loaded.tick() {
                    let _ = send(&mut writer, &Message::ParamChanged { instance, id, value });
                }
                loaded.unload();
                suhina_plugins::trace!("host unloaded {instance} in {:?}", began.elapsed());
            }
            Message::Unloaded { instance }
        }
        Message::OpenEditor { instance, owner, embedded } => {
            let mode = match (embedded, owner) {
                (true, Some(parent)) => window::Mode::Embedded { parent },
                (true, None) => return Some(failed(instance, "no window to embed the editor in".into())),
                (false, owner) => {
                    window::set_owner(owner);
                    window::Mode::Floating
                }
            };
            match instances.get_mut(&instance).map(|loaded| loaded.open_editor(mode)) {
                Some(Ok(size)) => Message::EditorOpened { instance, width: size.width, height: size.height },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::PlaceEditor { instance, placement } => {
            if let Some(loaded) = instances.get(&instance) {
                loaded.place_editor(placement);
            }
            return None;
        }
        Message::AraUpdate { instance, document } => {
            match instances.get_mut(&instance).map(|l| l.ara_update(&document)) {
                Some(Ok(())) => Message::AraUpdated { instance },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::AraExport { instance, modification, source } => {
            match instances.get(&instance).map(|l| l.ara_export(&modification, &source)) {
                Some(Ok((archive_id, data))) => Message::AraExported { instance, archive_id, data },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::AraImport { instance, archive_id, data, from, to, source } => {
            match instances.get(&instance).map(|l| l.ara_import(&archive_id, &data, &from, &to, &source)) {
                Some(Ok(())) => Message::AraImported { instance },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::FitEditor { instance, width, height } => {
            match instances.get_mut(&instance) {
                Some(Loaded::Vst3(plugin)) => plugin.fit_editor(window::Size { width, height }),
                Some(Loaded::Clap(plugin)) => plugin.fit_editor(window::Size { width, height }),
                #[cfg(feature = "vst2")]
                Some(Loaded::Vst2(plugin)) => plugin.fit_editor(window::Size { width, height }),
                _ => {}
            }
            return None;
        }
        Message::AraSelect { instance, region } => {
            if let Some(loaded) = instances.get(&instance) {
                loaded.ara_select(region.as_deref());
            }
            return None;
        }
        Message::CloseEditor { instance } => {
            if let Some(loaded) = instances.get_mut(&instance) {
                loaded.close_editor();
            }
            Message::EditorClosed { instance }
        }
        Message::GetParams { instance } => match instances.get_mut(&instance).map(Loaded::params) {
            Some(Ok(params)) => Message::Params { instance, params },
            Some(Err(message)) => failed(instance, message),
            None => missing(instance),
        },
        Message::SetParam { instance, id, value } => {
            match instances.get_mut(&instance).map(|p| p.set_param(id, value)) {
                Some(Ok(param)) => Message::Param { instance, param },
                Some(Err(message)) => failed(instance, message),
                None => missing(instance),
            }
        }
        Message::GetState { instance } => match instances.get_mut(&instance).map(Loaded::state) {
            Some(Ok((data, controller))) => Message::State { instance, data, controller },
            Some(Err(message)) => failed(instance, message),
            None => missing(instance),
        },
        Message::SetState { instance, data, controller } => match instances.get_mut(&instance) {
            Some(plugin) => match plugin.set_state(&data, controller.as_deref()).and_then(|()| plugin.state()) {
                Ok((data, controller)) => Message::State { instance, data, controller },
                Err(message) => failed(instance, message),
            },
            None => missing(instance),
        },
        other => Message::Error { instance: other.instance(), message: format!("unexpected message {other:?}") },
    })
}
