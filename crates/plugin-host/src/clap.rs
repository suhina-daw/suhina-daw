// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running a CLAP plug-in (through `clack-host`).
//!
//! The plug-in instance lives on the host's main thread, which also answers control messages
//! and serves the plug-in's main-thread callbacks; the started audio processor lives on the
//! audio thread, which serves blocks from the link. Every audio port the plug-in declares
//! (`audio-ports`) gets a buffer with the channels it declared, as CLAP requires: a plug-in
//! may read every channel of every port. The link's channels go to the main input port's first
//! channels and come from the main output port's (a mono one feeds them all); the other
//! channels and ports get silence. A plug-in without the extension gets one input and one
//! output port with the link's channels. Notes go to note port 0 as note events; controllers
//! and pitch bend as MIDI events. The host serves the plug-in's timers on every system and, on Unix, the
//! descriptors it waits on (`timer-support`, `posix-fd-support`; [`crate::run_loop`]), from its
//! main loop. Parameters are read on the main thread; a value set goes to the audio thread
//! and reaches the plug-in as a parameter event at the start of the next block.
//!
//! A plug-in that asks for a restart (`request_restart`, as one whose latency changes while it
//! runs must) is restarted from the main loop: its audio thread stops, it is deactivated and
//! activated again, and a new audio thread serves the same link. Its latency is read again then,
//! and when it says `latency.changed`.
//!
//! A plug-in is named `clap:<file>` (the first plug-in in the file) or `clap:<file>#<id>`, or
//! `test:clap` for the gain plug-in built into the host for tests ([`crate::test_clap`]), or
//! `test:clap-mix` for the same gain mixing its first two channels into the first, or
//! `test:clap-delay` for the delay whose latency is a parameter ([`crate::test_clap_delay`]), or
//! `test:clap-ports` for the one with a rack's port layout ([`crate::test_clap_ports`]).

use std::ffi::CString;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl, PluginPosixFd};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::events::Match;
use clack_host::events::event_types::{
    MidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent, TransportEvent, TransportFlags,
};
use clack_host::events::{EventFlags, EventHeader};
use clack_host::prelude::*;
use clack_host::utils::{BeatTime, SecondsTime};
use suhina_ipc::{Event, EventKind, MAX_EVENTS, Served};
use suhina_plugins::ParamInfo;

/// Output events the audio thread has room for each block.
const EVENT_CAPACITY: usize = 512;

pub(crate) struct Host;

pub(crate) struct Shared {
    callback_requested: AtomicBool,
    /// The plug-in asked to be deactivated and activated again (from any thread).
    restart_requested: AtomicBool,
    state: OnceLock<Option<PluginState>>,
    latency: OnceLock<Option<PluginLatency>>,
    audio_ports: OnceLock<Option<PluginAudioPorts>>,
    timer: OnceLock<Option<PluginTimer>>,
    #[cfg(unix)]
    posix_fd: OnceLock<Option<PluginPosixFd>>,
    params: OnceLock<Option<PluginParams>>,
    gui: OnceLock<Option<PluginGui>>,
    /// A size the plug-in asked its editor's window to have (from any thread).
    gui_resize: Mutex<Option<GuiSize>>,
    /// The plug-in said its editor is gone.
    gui_closed: AtomicBool,
    /// ... and that it destroyed it itself, so the host must not.
    gui_destroyed: AtomicBool,
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        if let Ok(mut wanted) = self.gui_resize.lock() {
            *wanted = Some(new_size);
        }
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, was_destroyed: bool) {
        self.gui_destroyed.store(was_destroyed, Ordering::Release);
        self.gui_closed.store(true, Ordering::Release);
    }
}

/// An open editor: the window the plug-in's view is in.
struct ClapEditor {
    window: crate::window::Window,
    size: crate::window::Size,
    /// The user may resize the window (a floating one the plug-in allows it for).
    resizable: bool,
    /// A size the editor got since the engine was last told, by the plug-in or the user.
    resized: Option<crate::window::Size>,
}

/// How the editor is embedded on this system: Win32 on Windows, Cocoa on macOS (there is no
/// X11 on a Mac), X11 elsewhere.
const GUI: GuiConfiguration<'static> = GuiConfiguration {
    api_type: if cfg!(windows) {
        GuiApiType::WIN32
    } else if cfg!(target_os = "macos") {
        GuiApiType::COCOA
    } else {
        GuiApiType::X11
    },
    is_floating: false,
};

/// Most audio ports one direction may have, and most channels one port may have: a plug-in's
/// numbers are not trusted.
const MAX_PORTS: usize = 16;
const MAX_PORT_CHANNELS: u32 = 64;

/// A plug-in's audio ports in one direction: how many channels each has, and which one
/// carries the link's audio (`None` without ports).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ports {
    channels: Vec<u32>,
    main: Option<usize>,
}

impl Ports {
    /// The ports as declared, each as its channels and whether it is the main one (capped).
    /// The main port is the one flagged so, else the first (CLAP puts it first).
    fn declared(ports: impl IntoIterator<Item = (u32, bool)>) -> Self {
        let ports: Vec<(u32, bool)> =
            ports.into_iter().take(MAX_PORTS).map(|(channels, main)| (channels.min(MAX_PORT_CHANNELS), main)).collect();
        let main = ports.iter().position(|(_, main)| *main).or((!ports.is_empty()).then_some(0));
        Self { channels: ports.into_iter().map(|(channels, _)| channels).collect(), main }
    }

    /// The plug-in's ports in one direction: as it declares them, or one port with the link's
    /// channels when it has no `audio-ports` extension.
    fn of(instance: &mut PluginInstance<Host>, is_input: bool, link_channels: u32) -> Self {
        let Some(ports) = instance.access_shared_handler(|s| s.audio_ports.get().copied().flatten()) else {
            return Self::declared([(link_channels, true)]);
        };
        let handle = instance.plugin_handle();
        let count = ports.count(&handle, is_input).min(MAX_PORTS as u32);
        Self::declared((0..count).map(|index| {
            let mut buffer = AudioPortInfoBuffer::new();
            match ports.get(&handle, index, is_input, &mut buffer) {
                Some(info) => (info.channel_count, info.flags.contains(AudioPortFlags::IS_MAIN)),
                // A port it cannot describe still gets a buffer, as it counted it.
                None => (0, false),
            }
        }))
    }

    fn total_channels(&self) -> usize {
        self.channels.iter().map(|c| *c as usize).sum()
    }

    /// One zeroed buffer of `frames` for each channel of each port.
    fn buffers(&self, frames: usize) -> Vec<Vec<Vec<f32>>> {
        self.channels.iter().map(|c| vec![vec![0.0f32; frames]; *c as usize]).collect()
    }
}

/// Parameter values waiting for the next block: id and plain value.
type PendingParams = Arc<Mutex<Vec<(u32, f64)>>>;
/// Most values one block carries; more wait for the next.
const MAX_PARAM_CHANGES: usize = 1024;

impl<'a> SharedHandler<'a> for Shared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.state.set(instance.get_extension());
        let _ = self.latency.set(instance.get_extension());
        let _ = self.audio_ports.set(instance.get_extension());
        let _ = self.timer.set(instance.get_extension());
        #[cfg(unix)]
        let _ = self.posix_fd.set(instance.get_extension());
        let _ = self.params.set(instance.get_extension());
        let _ = self.gui.set(instance.get_extension());
    }

    fn request_restart(&self) {
        self.restart_requested.store(true, Ordering::Release);
    }

    fn request_process(&self) {}

    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

impl HostLogImpl for Shared {
    fn log(&self, severity: LogSeverity, message: &str) {
        eprintln!("plug-in {severity:?}: {message}");
    }
}

/// The plug-in's timers and (on Unix) the descriptors it waits on, served from the main loop.
pub(crate) struct MainThread {
    /// The plug-in said its latency changed (`latency.changed`).
    latency_changed: std::cell::Cell<bool>,
    timers: std::cell::RefCell<crate::run_loop::Timers<u32>>,
    next_timer: std::cell::Cell<u32>,
    #[cfg(unix)]
    fds: std::cell::RefCell<Vec<(i32, FdFlags)>>,
}

impl MainThread {
    fn new() -> Self {
        Self {
            latency_changed: std::cell::Cell::new(false),
            timers: std::cell::RefCell::new(crate::run_loop::Timers::new()),
            next_timer: std::cell::Cell::new(0),
            #[cfg(unix)]
            fds: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl HostTimerImpl for MainThread {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        let id = self.next_timer.get();
        self.next_timer.set(id.wrapping_add(1));
        let added = self
            .timers
            .try_borrow_mut()
            .is_ok_and(|mut t| t.add(id, std::time::Duration::from_millis(u64::from(period_ms))));
        if added { Ok(TimerId(id)) } else { Err(HostError::Message("too many timers")) }
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        let mut timers = self.timers.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        timers.remove(|id| *id == timer_id.0);
        Ok(())
    }
}

#[cfg(unix)]
impl HostPosixFdImpl for MainThread {
    fn register_fd(&self, fd: i32, flags: FdFlags) -> Result<(), HostError> {
        let mut fds = self.fds.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        if fds.len() >= 64 {
            return Err(HostError::Message("too many descriptors"));
        }
        fds.retain(|(kept, _)| *kept != fd);
        fds.push((fd, flags));
        Ok(())
    }

    fn modify_fd(&self, fd: i32, flags: FdFlags) -> Result<(), HostError> {
        let mut fds = self.fds.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        let slot = fds.iter_mut().find(|(kept, _)| *kept == fd).ok_or(HostError::Message("not registered"))?;
        slot.1 = flags;
        Ok(())
    }

    fn unregister_fd(&self, fd: i32) -> Result<(), HostError> {
        let mut fds = self.fds.try_borrow_mut().map_err(|_| HostError::Message("busy"))?;
        fds.retain(|(kept, _)| *kept != fd);
        Ok(())
    }
}

impl MainThreadHandler<'_> for MainThread {}

impl HostStateImpl for MainThread {
    fn mark_dirty(&self) {}
}

impl HostLatencyImpl for MainThread {
    fn changed(&self) {
        self.latency_changed.set(true);
    }
}

impl HostHandlers for Host {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostState>()
            .register::<HostLatency>()
            .register::<HostGui>()
            .register::<HostTimer>();
        #[cfg(unix)]
        builder.register::<HostPosixFd>();
    }
}

/// What the audio thread hands back when it stops: the processor, to deactivate, and the link,
/// for the next audio thread after a restart.
type Stopped = (StoppedPluginAudioProcessor<Host>, Served);

/// A loaded, active CLAP plug-in and its audio thread.
pub(crate) struct ClapPlugin {
    instance: PluginInstance<Host>,
    audio: Option<thread::JoinHandle<Option<Stopped>>>,
    /// Its number in the process, which its audio thread serves as.
    number: u32,
    stop: Arc<AtomicBool>,
    latency: u32,
    /// How it was activated, to activate it the same way after a restart.
    configuration: PluginAudioConfiguration,
    /// Its audio ports (inputs, outputs), as they were when it was activated.
    ports: (Ports, Ports),
    pending: PendingParams,
    editor: Option<ClapEditor>,
    /// The plug-in's name, for its editor's window.
    name: String,
}

/// The entry `spec` names: a CLAP file, or the built-in test plug-in. Also returns the plug-in
/// id asked for, if any.
fn entry(spec: &str) -> Result<(PluginEntry, Option<String>), String> {
    if spec == "test:clap" {
        let entry = PluginEntry::load_from_clack::<crate::test_clap::Entry>(c"suhina-test-clap")
            .map_err(|e| format!("test plug-in: {e}"))?;
        return Ok((entry, None));
    }
    if spec == "test:clap-mix" {
        let entry = PluginEntry::load_from_clack::<crate::test_clap::MixEntry>(c"suhina-test-clap-mix")
            .map_err(|e| format!("test plug-in: {e}"))?;
        return Ok((entry, None));
    }
    if spec == "test:clap-delay" {
        let entry = PluginEntry::load_from_clack::<crate::test_clap_delay::Entry>(c"suhina-test-clap-delay")
            .map_err(|e| format!("test plug-in: {e}"))?;
        return Ok((entry, None));
    }
    if spec == "test:clap-ports" {
        let entry = PluginEntry::load_from_clack::<crate::test_clap_ports::Entry>(c"suhina-test-clap-ports")
            .map_err(|e| format!("test plug-in: {e}"))?;
        return Ok((entry, None));
    }
    let file = spec.strip_prefix("clap:").ok_or_else(|| format!("not a CLAP plug-in: {spec:?}"))?;
    let (path, id) = match file.rsplit_once('#') {
        Some((path, id)) if !id.is_empty() => (path, Some(id.to_string())),
        _ => (file, None),
    };
    // SAFETY: loading a CLAP file runs its initialisation code, which Rust cannot check. That is
    // the job of this process: it exists so that whatever the plug-in does, it does it here and
    // not in the engine (ADR 0006). The entry keeps the library loaded while any instance lives.
    let entry = unsafe { PluginEntry::load(path) }.map_err(|e| format!("could not load the plug-in file: {e}"))?;
    Ok((entry, id))
}

/// The plug-ins the file `spec` names holds, without instantiating any (for scanning).
pub(crate) fn describe(spec: &str) -> Result<Vec<suhina_plugins::scan::PluginInfo>, String> {
    let (entry, _) = entry(spec)?;
    let factory = entry.get_plugin_factory().ok_or("the file has no plug-in factory")?;
    let text = |c: Option<&std::ffi::CStr>| c.map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(factory
        .plugin_descriptors()
        .map(|d| suhina_plugins::scan::PluginInfo {
            path: std::path::PathBuf::new(),
            id: text(d.id()),
            name: text(d.name()),
            vendor: text(d.vendor()),
            version: text(d.version()),
            features: d.features().map(|f| f.to_string_lossy().into_owned()).collect(),
            replaces: Vec::new(),
            ara: false,
        })
        .collect())
}

impl ClapPlugin {
    /// Loads the plug-in `spec` names, activates it for the link's block size at `sample_rate`
    /// and starts serving blocks from `served` on an audio thread, as instance `number` of the
    /// process.
    pub(crate) fn load(spec: &str, number: u32, served: Served, sample_rate: f64) -> Result<Self, String> {
        let (entry, wanted) = entry(spec)?;
        let factory = entry.get_plugin_factory().ok_or("the file has no plug-in factory")?;
        let ids: Vec<CString> = factory.plugin_descriptors().filter_map(|d| d.id().map(CString::from)).collect();
        let id = match &wanted {
            Some(wanted) => ids.into_iter().find(|id| id.to_bytes() == wanted.as_bytes()),
            None => ids.into_iter().next(),
        }
        .ok_or_else(|| format!("no plug-in {} in {spec:?}", wanted.as_deref().unwrap_or("at all")))?;

        let info =
            HostInfo::new("Suhina", "Suhina", "https://github.com/suhina-daw/suhina-daw", env!("CARGO_PKG_VERSION"))
                .map_err(|e| e.to_string())?;
        let shared = |_: &()| Shared {
            callback_requested: AtomicBool::new(false),
            restart_requested: AtomicBool::new(false),
            state: OnceLock::new(),
            latency: OnceLock::new(),
            audio_ports: OnceLock::new(),
            timer: OnceLock::new(),
            #[cfg(unix)]
            posix_fd: OnceLock::new(),
            params: OnceLock::new(),
            gui: OnceLock::new(),
            gui_resize: Mutex::new(None),
            gui_closed: AtomicBool::new(false),
            gui_destroyed: AtomicBool::new(false),
        };
        let mut instance = PluginInstance::<Host>::new(shared, |_| MainThread::new(), &entry, &id, &info)
            .map_err(|e| e.to_string())?;

        let max_frames = served.max_frames();
        let configuration = PluginAudioConfiguration { sample_rate, min_frames_count: 1, max_frames_count: max_frames };
        let stopped = instance.activate(|_, _| (), configuration).map_err(|e| e.to_string())?;
        let latency = read_latency(&mut instance);

        let ports =
            (Ports::of(&mut instance, true, served.channels()), Ports::of(&mut instance, false, served.channels()));

        let name = String::from_utf8_lossy(id.to_bytes()).into_owned();
        let mut plugin = Self {
            instance,
            audio: None,
            stop: Arc::new(AtomicBool::new(false)),
            latency,
            configuration,
            ports,
            number,
            pending: PendingParams::default(),
            editor: None,
            name,
        };
        plugin.start_audio(stopped, served)?;
        Ok(plugin)
    }

    /// Starts an audio thread that serves blocks from `served` through the activated plug-in.
    fn start_audio(&mut self, stopped: StoppedPluginAudioProcessor<Host>, served: Served) -> Result<(), String> {
        self.stop = Arc::new(AtomicBool::new(false));
        let (audio_stop, audio_pending, ports, number) =
            (self.stop.clone(), self.pending.clone(), self.ports.clone(), self.number);
        let sample_rate = self.configuration.sample_rate;
        let audio = thread::Builder::new()
            .name("plug-in audio".into())
            .spawn(move || {
                let _realtime = crate::real_time_thread();
                serve(number, stopped, served, ports, (audio_pending, sample_rate), &audio_stop)
            })
            .map_err(|e| e.to_string())?;
        self.audio = Some(audio);
        Ok(())
    }

    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    /// A latency the plug-in reported since this was last asked. A plug-in that asked for a
    /// restart is restarted first (its latency may change only while it is deactivated).
    pub(crate) fn latency_changed(&mut self) -> Option<u32> {
        let restart = self.instance.access_shared_handler(|s| s.restart_requested.swap(false, Ordering::AcqRel));
        let changed = self.instance.access_handler(|m| m.latency_changed.replace(false));
        if !restart && !changed {
            return None;
        }
        if restart && let Err(why) = self.reactivate() {
            eprintln!("plug-in {}: could not restart it: {why}", self.name);
        }
        let latency = read_latency(&mut self.instance);
        (latency != self.latency).then(|| {
            self.latency = latency;
            latency
        })
    }

    /// Stops the audio thread, deactivates the plug-in, activates it again as it was and starts
    /// a new audio thread on the same link. An audio thread stuck in the plug-in is left as it
    /// is, and so is the plug-in.
    fn reactivate(&mut self) -> Result<(), String> {
        if !self.stop_audio(crate::UNLOAD_WAIT) {
            return Err("its audio thread does not stop".into());
        }
        let (stopped, served) = self
            .audio
            .take()
            .ok_or("it has no audio thread")?
            .join()
            .ok()
            .flatten()
            .ok_or("its audio thread failed")?;
        self.instance.deactivate(stopped);
        let stopped = self.instance.activate(|_, _| (), self.configuration).map_err(|e| e.to_string())?;
        // What it said while activating is what is read next.
        self.instance.access_handler(|m| m.latency_changed.set(false));
        // A restart may also be for ports it changed.
        self.ports = (
            Ports::of(&mut self.instance, true, served.channels()),
            Ports::of(&mut self.instance, false, served.channels()),
        );
        self.start_audio(stopped, served)
    }

    /// Asks the audio thread to stop and waits up to `wait` for it; whether it stopped.
    pub(crate) fn stop_audio(&mut self, wait: Duration) -> bool {
        crate::audio_stopped(&self.stop, self.audio.as_ref(), wait)
    }

    /// Runs the plug-in's main-thread callback if it asked for one.
    pub(crate) fn tick(&mut self) {
        if self.instance.access_shared_handler(|s| s.callback_requested.swap(false, Ordering::AcqRel)) {
            self.instance.call_on_main_thread_callback();
        }
        self.run_loop();
    }

    /// Calls the plug-in back for the timers that are due and (on Unix) the descriptors that
    /// are ready. The lists are copied first: a callback may register or unregister.
    fn run_loop(&mut self) {
        let due = self.instance.access_handler(|m| {
            m.timers.try_borrow_mut().map(|mut t| t.due(std::time::Instant::now())).unwrap_or_default()
        });
        if let Some(timer) = self.instance.access_shared_handler(|s| s.timer.get().copied().flatten()) {
            let handle = self.instance.plugin_handle();
            for id in due {
                timer.on_timer(&handle, TimerId(id));
            }
        }
        #[cfg(unix)]
        {
            let fds = self.instance.access_handler(|m| m.fds.try_borrow().map(|f| f.clone()).unwrap_or_default());
            let wanted: Vec<i32> = fds.iter().map(|(fd, _)| *fd).collect();
            if let Some(posix_fd) = self.instance.access_shared_handler(|s| s.posix_fd.get().copied().flatten()) {
                let handle = self.instance.plugin_handle();
                for fd in crate::run_loop::ready(&wanted) {
                    posix_fd.on_fd(&handle, fd, FdFlags::READ);
                }
            }
        }
    }

    pub(crate) fn state(&mut self) -> Result<Vec<u8>, String> {
        let state = self.state_extension()?;
        let mut data = Vec::new();
        state.save(&self.instance.plugin_handle(), &mut data).map_err(|e| e.to_string())?;
        Ok(data)
    }

    pub(crate) fn set_state(&mut self, data: &[u8]) -> Result<(), String> {
        let state = self.state_extension()?;
        state.load(&self.instance.plugin_handle(), &mut std::io::Cursor::new(data)).map_err(|e| e.to_string())
    }

    fn gui_extension(&self) -> Option<PluginGui> {
        self.instance.access_shared_handler(|s| s.gui.get().copied().flatten())
    }

    /// Opens the plug-in's editor as `mode` says (or returns the size of the one open).
    pub(crate) fn open_editor(&mut self, mode: crate::window::Mode) -> Result<crate::window::Size, String> {
        if let Some(editor) = &self.editor {
            return Ok(editor.size);
        }
        let gui = self.gui_extension().ok_or("the plug-in has no editor")?;
        self.instance.access_shared_handler(|s| {
            s.gui_closed.store(false, Ordering::Release);
            s.gui_destroyed.store(false, Ordering::Release);
        });
        let handle = self.instance.plugin_handle();
        if !gui.is_api_supported(&handle, GUI) {
            return Err("the plug-in's editor does not run on this system".into());
        }
        gui.create(&handle, GUI).map_err(|_| "the plug-in's editor did not open")?;
        let size = gui
            .get_size(&handle)
            .map(|s| crate::window::Size { width: s.width.clamp(1, 16_384), height: s.height.clamp(1, 16_384) })
            .unwrap_or(crate::window::Size { width: 800, height: 600 });
        let resizable = gui.can_resize(&handle) && mode == crate::window::Mode::Floating;
        let window = match crate::window::Window::open_as(mode, &self.name, size, resizable) {
            Ok(window) => window,
            Err(e) => {
                gui.destroy(&handle);
                return Err(e);
            }
        };
        let parent = parent_window(&window);
        // SAFETY: as above.
        if unsafe { gui.set_parent(&handle, parent) }.is_err() {
            gui.destroy(&handle);
            return Err("the plug-in's editor did not open".into());
        }
        let _ = gui.show(&handle);
        self.editor = Some(ClapEditor { window, size, resizable, resized: None });
        Ok(size)
    }

    /// Puts an embedded editor where the engine says.
    pub(crate) fn place_editor(&self, placement: crate::window::Placement) {
        if let Some(editor) = &self.editor {
            crate::window::place(editor.window.id(), placement);
        }
    }

    /// Gives an embedded editor the room the engine has for it, when the plug-in can be resized:
    /// the size the plug-in takes nearest to it (#689).
    pub(crate) fn fit_editor(&mut self, size: crate::window::Size) {
        let Some(gui) = self.gui_extension() else { return };
        let handle = self.instance.plugin_handle();
        let Some(editor) = &mut self.editor else { return };
        if size.width == 0 || size.height == 0 || !gui.can_resize(&handle) {
            return;
        }
        let asked = GuiSize { width: size.width, height: size.height };
        let fitted = gui.adjust_size(&handle, asked).unwrap_or(asked);
        let got = crate::window::Size { width: fitted.width.clamp(1, 16_384), height: fitted.height.clamp(1, 16_384) };
        if got == editor.size || gui.set_size(&handle, fitted).is_err() {
            return;
        }
        crate::window::resize(editor.window.id(), got);
        editor.size = got;
        editor.resized = Some(got);
    }

    /// A size the editor got since this was last asked.
    pub(crate) fn editor_resized(&mut self) -> Option<crate::window::Size> {
        self.editor.as_mut().and_then(|e| e.resized.take())
    }

    pub(crate) fn close_editor(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        let destroyed = self.instance.access_shared_handler(|s| s.gui_destroyed.swap(false, Ordering::AcqRel));
        if let Some(gui) = self.gui_extension().filter(|_| !destroyed) {
            let handle = self.instance.plugin_handle();
            let _ = gui.hide(&handle);
            gui.destroy(&handle);
        }
        drop(editor.window);
    }

    /// Keeps the editor in step with its window; returns whether it was closed.
    pub(crate) fn editor_tick(&mut self) -> bool {
        if self.editor.is_none() {
            return false;
        }
        let gui = self.gui_extension();
        let wanted = self.instance.access_shared_handler(|s| s.gui_resize.lock().ok().and_then(|mut w| w.take()));
        let handle = self.instance.plugin_handle();
        let Some(editor) = &mut self.editor else { return false };
        if let Some(size) = wanted {
            let size = crate::window::Size { width: size.width.clamp(1, 16_384), height: size.height.clamp(1, 16_384) };
            crate::window::resize(editor.window.id(), size);
            editor.size = size;
            editor.resized = Some(size);
        } else if editor.resizable {
            let now = crate::window::client_size(editor.window.id());
            if now != editor.size && now.width > 0 && now.height > 0 {
                if let Some(gui) = gui {
                    let asked = GuiSize { width: now.width, height: now.height };
                    let fitted = gui.adjust_size(&handle, asked).unwrap_or(asked);
                    let _ = gui.set_size(&handle, fitted);
                }
                editor.size = now;
                editor.resized = Some(now);
            }
        }
        let gone = self.instance.access_shared_handler(|s| s.gui_closed.swap(false, Ordering::AcqRel));
        let asked = self.editor.as_ref().is_some_and(|e| crate::window::take_close_request(e.window.id()));
        let closed = asked || gone;
        if closed {
            self.close_editor();
        }
        closed
    }

    fn params_extension(&self) -> Option<PluginParams> {
        self.instance.access_shared_handler(|s| s.params.get().copied().flatten())
    }

    /// The plug-in's parameters, values in their plain ranges.
    pub(crate) fn params(&mut self) -> Vec<ParamInfo> {
        let Some(params) = self.params_extension() else { return Vec::new() };
        let handle = self.instance.plugin_handle();
        let count = params.count(&handle).min(1 << 16);
        (0..count)
            .filter_map(|index| {
                let mut buffer = ParamInfoBuffer::new();
                let info = params.get_info(&handle, index, &mut buffer)?;
                let value = params.get_value(&handle, info.id).unwrap_or(info.default_value);
                Some(describe_param(&params, &handle, &info, value))
            })
            .collect()
    }

    /// Sets parameter `id` to `value` (kept within its range); the plug-in gets it at the start
    /// of the next block.
    pub(crate) fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        let params = self.params_extension().ok_or("the plug-in has no parameters")?;
        let handle = self.instance.plugin_handle();
        let count = params.count(&handle).min(1 << 16);
        let mut buffer = ParamInfoBuffer::new();
        let info = (0..count)
            .find_map(|index| {
                let mut probe = ParamInfoBuffer::new();
                let found = params.get_info(&handle, index, &mut probe)?.id.get() == id;
                found.then_some(index)
            })
            .and_then(|index| params.get_info(&handle, index, &mut buffer))
            .ok_or_else(|| format!("no parameter {id}"))?;
        let (low, high) = (crate::vst3::finite(info.min_value, 0.0), crate::vst3::finite(info.max_value, 1.0));
        let value = value.clamp(low.min(high), high.max(low));
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|(other, _)| *other != id);
            pending.push((id, value));
        }
        Ok(describe_param(&params, &handle, &info, value))
    }

    fn state_extension(&self) -> Result<PluginState, String> {
        self.instance
            .access_shared_handler(|s| s.state.get().copied().flatten())
            .ok_or_else(|| "the plug-in keeps no state".to_string())
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        self.close_editor();
        self.stop.store(true, Ordering::Release);
        if let Some(audio) = self.audio.take()
            && let Ok(Some((stopped, _served))) = audio.join()
        {
            self.instance.deactivate(stopped);
        }
    }
}

/// The plug-in's latency as it reports it now (0 without the extension).
fn read_latency(instance: &mut PluginInstance<Host>) -> u32 {
    match instance.access_shared_handler(|s| s.latency.get().copied().flatten()) {
        Some(latency) => latency.get(&instance.plugin_handle()),
        None => 0,
    }
}

/// The audio thread: serves blocks from the link through the plug-in until `stop` is set (and
/// runs it after other instances when their blocks ask, [`crate::audio`]), and hands the
/// stopped processor back for deactivation on the main thread, with the link.
fn serve(
    instance: u32,
    stopped: StoppedPluginAudioProcessor<Host>,
    mut served: Served,
    (input_ports, output_ports): (Ports, Ports),
    (pending, sample_rate): (PendingParams, f64),
    stop: &AtomicBool,
) -> Option<Stopped> {
    let processor = match stopped.start_processing() {
        Ok(started) => started,
        Err(e) => return Some((e.into_stopped_processor(), served)),
    };
    let block = ClapBlock::new(processor, &served, (input_ports, output_ports), pending, sample_rate);
    crate::audio::serve(instance, block, &mut served, stop).map(|block| (block.processor.stop_processing(), served))
}

/// The started plug-in and what it needs to run a block, without allocating.
struct ClapBlock {
    processor: StartedPluginAudioProcessor<Host>,
    channels: usize,
    input_ports: Ports,
    output_ports: Ports,
    inputs: Vec<Vec<Vec<f32>>>,
    outputs: Vec<Vec<Vec<f32>>>,
    input_audio: AudioPorts,
    output_audio: AudioPorts,
    in_events: EventBuffer,
    out_events: EventBuffer,
    pending: PendingParams,
    steady_time: u64,
    /// Where the song is, as the engine last said: given with every block.
    transport: TransportEvent,
    sample_rate: f64,
}

/// The transport before the engine says where the song is: stopped at the start, 120 BPM, 4/4.
fn blank_transport() -> TransportEvent {
    TransportEvent {
        header: EventHeader::new_core(0, EventFlags::empty()),
        flags: TransportFlags::HAS_TEMPO
            | TransportFlags::HAS_BEATS_TIMELINE
            | TransportFlags::HAS_SECONDS_TIMELINE
            | TransportFlags::HAS_TIME_SIGNATURE,
        song_pos_beats: BeatTime::from_float(0.0),
        song_pos_seconds: SecondsTime::from_float(0.0),
        tempo: 120.0,
        tempo_inc: 0.0,
        loop_start_beats: BeatTime::from_float(0.0),
        loop_end_beats: BeatTime::from_float(0.0),
        loop_start_seconds: SecondsTime::from_float(0.0),
        loop_end_seconds: SecondsTime::from_float(0.0),
        bar_start: BeatTime::from_float(0.0),
        bar_number: 0,
        time_signature_numerator: 4,
        time_signature_denominator: 4,
    }
}

/// Takes the song's place the engine sent with a block into `transport`.
fn follow_song(transport: &mut TransportEvent, events: &[Event], sample_rate: f64) {
    for event in events {
        match event.kind {
            EventKind::Transport { playing, position } => {
                transport.flags.set(TransportFlags::IS_PLAYING, playing);
                if sample_rate > 0.0 {
                    transport.song_pos_seconds = SecondsTime::from_float(position as f64 / sample_rate);
                }
            }
            EventKind::Tempo { bpm } => transport.tempo = f64::from(bpm),
            EventKind::Beat { beat } => transport.song_pos_beats = BeatTime::from_float(beat),
            EventKind::Cycle { on, start, end } => {
                transport.flags.set(TransportFlags::IS_LOOP_ACTIVE, on);
                transport.loop_start_beats = BeatTime::from_float(f64::from(start));
                transport.loop_end_beats = BeatTime::from_float(f64::from(end));
            }
            EventKind::CycleSeconds { start, end } => {
                transport.loop_start_seconds = SecondsTime::from_float(f64::from(start));
                transport.loop_end_seconds = SecondsTime::from_float(f64::from(end));
            }
            EventKind::Bar { number, start, numerator, denominator } => {
                transport.bar_number = i32::try_from(number).unwrap_or(i32::MAX);
                transport.bar_start = BeatTime::from_float(start);
                transport.time_signature_numerator = u16::from(numerator);
                transport.time_signature_denominator = u16::from(denominator);
            }
            _ => {}
        }
    }
}

impl ClapBlock {
    fn new(
        processor: StartedPluginAudioProcessor<Host>,
        served: &Served,
        (input_ports, output_ports): (Ports, Ports),
        pending: PendingParams,
        sample_rate: f64,
    ) -> Self {
        let max_frames = served.max_frames() as usize;
        let inputs = input_ports.buffers(max_frames);
        let outputs = output_ports.buffers(max_frames);
        let input_audio = AudioPorts::with_capacity(input_ports.total_channels(), inputs.len());
        let output_audio = AudioPorts::with_capacity(output_ports.total_channels(), outputs.len());
        Self {
            processor,
            channels: served.channels() as usize,
            input_ports,
            output_ports,
            inputs,
            outputs,
            input_audio,
            output_audio,
            // Room for a block's parameter values and its events, so pushing never grows it.
            in_events: EventBuffer::with_capacity(MAX_EVENTS + MAX_PARAM_CHANGES),
            out_events: EventBuffer::with_capacity(EVENT_CAPACITY),
            pending,
            steady_time: 0,
            transport: blank_transport(),
            sample_rate,
        }
    }
}

impl crate::audio::Block for ClapBlock {
    fn process(&mut self, input: &[f32], events: &[Event], output: &mut [f32]) {
        let Self {
            processor,
            channels,
            input_ports,
            output_ports,
            inputs,
            outputs,
            input_audio,
            output_audio,
            in_events,
            out_events,
            pending,
            steady_time,
            transport,
            sample_rate,
        } = self;
        let channels = *channels;
        let frames = input.len() / channels.max(1);
        // The link's channels into the main input port, by the rule every format follows
        // ([`crate::audio::feed_main`]); silence elsewhere (a plug-in may have written to an input).
        for (port, buffers) in inputs.iter_mut().enumerate() {
            if input_ports.main == Some(port) {
                crate::audio::feed_main(input, channels, frames, buffers);
                continue;
            }
            for buffer in buffers.iter_mut() {
                buffer.iter_mut().take(frames).for_each(|sample| *sample = 0.0);
            }
        }
        let audio_in = input_audio.with_input_buffers(inputs.iter_mut().map(|port| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                port.iter_mut().map(|b| InputChannel::variable(b.get_mut(..frames).unwrap_or_default())),
            ),
        }));
        let mut audio_out = output_audio.with_output_buffers(outputs.iter_mut().map(|port| AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                port.iter_mut().map(|b| b.get_mut(..frames).unwrap_or_default()),
            ),
        }));
        out_events.clear();
        in_events.clear();
        // Parameter values first, at the block's start; then the notes, in time order.
        if let Ok(mut values) = pending.try_lock() {
            let room = values.len().min(MAX_PARAM_CHANGES);
            for (id, value) in values.drain(..room) {
                if let Some(id) = ClapId::from_raw(id) {
                    in_events.push(&ParamValueEvent::new(0, id, Pckn::match_all(), value));
                }
            }
        }
        for event in events {
            push_event(in_events, event);
        }
        follow_song(transport, events, *sample_rate);
        let processed = processor.process(
            &audio_in,
            &mut audio_out,
            &InputEvents::from_buffer(in_events),
            &mut OutputEvents::from_buffer(out_events),
            Some(*steady_time),
            Some(transport),
        );
        *steady_time = steady_time.wrapping_add(frames as u64);
        // A failed block stays silent (the output starts zeroed). The link's channels come from
        // the main output port, by the shared rule.
        let main = output_ports.main.and_then(|port| outputs.get(port)).filter(|_| processed.is_ok());
        if let Some(main) = main {
            crate::audio::collect_main(main, channels, frames, output);
        }
    }
}

/// The host's window as the parent CLAP's `gui` extension takes on this system.
#[cfg(windows)]
fn parent_window(window: &crate::window::Window) -> clack_extensions::gui::Window<'static, 'static> {
    // SAFETY: the window stays open until the editor is destroyed in `close_editor`.
    unsafe { clack_extensions::gui::Window::from_win32_hwnd(window.handle()) }
}

/// The host's window as the parent CLAP's `gui` extension takes on this system: the content
/// view's pointer, for the plug-in's view to attach beside it.
#[cfg(target_os = "macos")]
fn parent_window(window: &crate::window::Window) -> clack_extensions::gui::Window<'static, 'static> {
    // SAFETY: the view stays open until the editor is destroyed in `close_editor`.
    unsafe { clack_extensions::gui::Window::from_cocoa_nsview(window.handle()) }
}

/// The host's window as the parent CLAP's `gui` extension takes on this system: the X11 id.
#[cfg(not(any(windows, target_os = "macos")))]
fn parent_window(window: &crate::window::Window) -> clack_extensions::gui::Window<'static, 'static> {
    clack_extensions::gui::Window::from_x11_handle(window.handle() as usize as std::ffi::c_ulong)
}

/// A parameter as the protocol describes it, at `value`.
fn describe_param(
    params: &PluginParams,
    handle: &PluginMainThreadHandle,
    info: &clack_extensions::params::ParamInfo,
    value: f64,
) -> ParamInfo {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes.split(|b| *b == 0).next().unwrap_or_default()).into_owned();
    let mut shown = [0u8; 256];
    let shown = params.value_to_text(handle, info.id, value, &mut shown).map(|t| text(t)).unwrap_or_default();
    let stepped = info.flags.contains(ParamInfoFlags::IS_STEPPED);
    // A plug-in's numbers are not trusted: every one sent on is finite.
    let (min, max) = (crate::vst3::finite(info.min_value, 0.0), crate::vst3::finite(info.max_value, 1.0));
    let span = (max - min).abs();
    ParamInfo {
        id: info.id.get(),
        name: text(info.name),
        group: text(info.module),
        min,
        max,
        default: crate::vst3::finite(info.default_value, min),
        value: crate::vst3::finite(value, min),
        text: shown,
        steps: if stepped && span.is_finite() { span.round().min(f64::from(u32::MAX)) as u32 } else { 0 },
        automatable: info.flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
        read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
        hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
    }
}

/// One event from the link as a CLAP event: notes as note events on note port 0, controllers
/// and pitch bend as MIDI messages, parameter values as parameter events.
fn push_event(buffer: &mut EventBuffer, event: &Event) {
    let note_id = |note: i32| u32::try_from(note).map_or(Match::All, Match::Specific);
    match event.kind {
        EventKind::NoteOn { channel, key, velocity } => {
            let pckn = Pckn::new(0u16, u16::from(channel), u16::from(key), note_id(event.note));
            buffer.push(&NoteOnEvent::new(event.frame, pckn, f64::from(velocity)));
        }
        EventKind::NoteOff { channel, key, velocity } => {
            let pckn = Pckn::new(0u16, u16::from(channel), u16::from(key), note_id(event.note));
            buffer.push(&NoteOffEvent::new(event.frame, pckn, f64::from(velocity)));
        }
        EventKind::Controller { channel, number, value } => {
            let value = (value * 127.0).round().clamp(0.0, 127.0) as u8;
            buffer.push(&MidiEvent::new(event.frame, 0, [0xB0 | (channel & 0x0F), number & 0x7F, value]));
        }
        EventKind::Param { id, value } => {
            if let Some(id) = ClapId::from_raw(id) {
                buffer.push(&ParamValueEvent::new(event.frame, id, Pckn::match_all(), f64::from(value)));
            }
        }
        EventKind::PitchBend { channel, value } => {
            let bend = ((value + 1.0) / 2.0 * 16_383.0).round().clamp(0.0, 16_383.0) as u16;
            let (low, high) = ((bend & 0x7F) as u8, (bend >> 7) as u8);
            buffer.push(&MidiEvent::new(event.frame, 0, [0xE0 | (channel & 0x0F), low, high]));
        }
        // Channel pressure (aftertouch, #812): a two-byte MIDI message, the third byte unused.
        EventKind::Pressure { channel, value } => {
            let value = (value * 127.0).round().clamp(0.0, 127.0) as u8;
            buffer.push(&MidiEvent::new(event.frame, 0, [0xD0 | (channel & 0x0F), value, 0]));
        }
        // The song's place goes to the plug-in as the block's transport ([`follow_song`]).
        EventKind::Transport { .. }
        | EventKind::Tempo { .. }
        | EventKind::Beat { .. }
        | EventKind::Bar { .. }
        | EventKind::Cycle { .. }
        | EventKind::CycleSeconds { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_keep_their_layout_and_find_the_main_one() {
        // A rack: 8-channel main port, then a stereo side port.
        assert_eq!(Ports::declared([(8, true), (2, false)]), Ports { channels: vec![8, 2], main: Some(0) });
        // The main port is the flagged one; without a flag, the first.
        assert_eq!(Ports::declared([(2, false), (1, true)]).main, Some(1));
        assert_eq!(Ports::declared([(2, false), (2, false)]).main, Some(0));
        // No ports (an instrument's inputs): nothing to feed.
        assert_eq!(Ports::declared([]), Ports { channels: vec![], main: None });
    }

    #[test]
    fn ports_a_plugin_declares_are_capped() {
        let many = Ports::declared(std::iter::repeat_n((u32::MAX, false), 1_000));
        assert_eq!(many.channels.len(), MAX_PORTS);
        assert!(many.channels.iter().all(|c| *c == MAX_PORT_CHANNELS));
        assert_eq!(many.buffers(4).iter().flatten().count(), many.total_channels());
    }
    #[test]
    fn the_loop_reaches_a_plug_in_on_both_timelines_it_is_told_of() {
        // The transport says it has a seconds timeline, so the loop is on it too, not 0 to 0.
        let mut transport = blank_transport();
        let events = [Event::cycle(true, 4.0, 12.0), Event::cycle_seconds(2.0, 6.0)];
        follow_song(&mut transport, &events, 48_000.0);
        assert!(transport.flags.contains(TransportFlags::IS_LOOP_ACTIVE));
        assert!(transport.flags.contains(TransportFlags::HAS_SECONDS_TIMELINE));
        assert_eq!((transport.loop_start_beats.to_float(), transport.loop_end_beats.to_float()), (4.0, 12.0));
        assert_eq!((transport.loop_start_seconds.to_float(), transport.loop_end_seconds.to_float()), (2.0, 6.0));
    }
}
