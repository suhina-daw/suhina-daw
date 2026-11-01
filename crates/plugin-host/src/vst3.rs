// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running a VST3 plug-in, through the `vst3` bindings to the VST 3 SDK (MIT since 3.8).
//!
//! The component and its controller live on the host's main thread, the audio processor's
//! `process` calls on the audio thread, as the SDK asks. Audio is stereo in and out on the main
//! buses; other buses (side-chains, extra outputs) get silent buffers of their own, so plug-ins
//! that expect every bus to have one get it. The state handed to the engine is the component's
//! (the controller is brought in step with it on load).
//!
//! A plug-in whose latency changes while it runs says so through the component handler
//! (`restartComponent` with `kLatencyChanged`). The main loop then stops the audio thread (which
//! hands the processor the parameter values still waiting, in a block of no samples, so it knows
//! the value that changed its latency), deactivates and activates the component, reads its
//! latency again and starts a new audio thread on the same link.
//!
//! A plug-in is named `vst3:<bundle or file>` (the first audio class in it) or
//! `vst3:<bundle or file>#<class id>`, the class id as 32 hex digits; `test:vst3` is a gain
//! built into the host for tests ([`crate::test_vst3`]), and `test:vst3-mix` the same gain mixing
//! its first two channels into the first.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsStr, c_void};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use suhina_ipc::Served;
use suhina_plugins::ParamInfo;
use suhina_plugins::scan::PluginInfo;

use crate::param_notes::{Notes, ParamNotes, Reads};
use crate::vst3_events::{BlockEvents, MidiMapping, Shown};
use crate::vst3_params::{BlockChanges, Pending};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// The category of the classes that process audio.
const AUDIO_CLASS: &str = "Audio Module Class";
/// The category of the class that lists which older plug-ins the module replaces.
const COMPATIBILITY_CLASS: &str = "Plugin Compatibility Class";
/// Bytes of compatibility list read, at most.
const MAX_COMPATIBILITY: usize = 1024 * 1024;

/// The byte order of a class id's canonical text form. On Windows the SDK lays ids out as COM
/// GUIDs, whose first three fields (4, 2 and 2 bytes) are stored little-endian, and its text
/// form (as in `moduleinfo.json`) writes those fields as numbers; elsewhere the 16 bytes are
/// written in the order they are stored.
const TEXT_ORDER: [usize; 16] = if cfg!(windows) {
    [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15]
} else {
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
};

/// A class id as the 32 hex digits the SDK, `moduleinfo.json` and the model use.
pub(crate) fn cid_hex(cid: &TUID) -> String {
    TEXT_ORDER.iter().filter_map(|&i| cid.get(i)).map(|b| format!("{:02X}", *b as u8)).collect()
}

fn cid_from_hex(hex: &str) -> Option<TUID> {
    let (pairs, rest) = hex.as_bytes().as_chunks::<2>();
    if pairs.len() != 16 || !rest.is_empty() {
        return None;
    }
    let mut cid: TUID = [0; 16];
    for (&index, pair) in TEXT_ORDER.iter().zip(pairs) {
        let text = std::str::from_utf8(pair).ok()?;
        *cid.get_mut(index)? = u8::from_str_radix(text, 16).ok()? as _;
    }
    Some(cid)
}

/// `value`, or `otherwise` when it is NaN or infinite (a plug-in's numbers are not trusted).
pub(crate) fn finite(value: f64, otherwise: f64) -> f64 {
    if value.is_finite() { value } else { otherwise }
}

/// Text from a fixed-size UTF-16 field (`String128`), up to its first zero.
fn utf16(field: &[TChar]) -> String {
    let units: Vec<u16> = field.iter().take_while(|c| **c != 0).copied().collect();
    String::from_utf16_lossy(&units)
}

/// Text from a fixed-size C string field.
fn text(field: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = field.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn ok(result: tresult) -> bool {
    result == kResultOk
}

/// A loaded plug-in file (or the built-in test plug-in) and its factory.
struct ModuleFile {
    factory: Option<ComPtr<IPluginFactory>>,
    library: Option<libloading::Library>,
}

/// The file inside a VST3 bundle that holds the code for this system.
fn binary(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    let name = path.file_name().ok_or("not a VST3 bundle")?;
    let stem = path.file_stem().ok_or("not a VST3 bundle")?;
    let contents = path.join("Contents");
    let inner = if cfg!(windows) {
        contents.join("x86_64-win").join(name)
    } else if cfg!(target_os = "linux") {
        contents.join("x86_64-linux").join(stem).with_extension("so")
    } else {
        return macos_binary(&contents.join("MacOS"), stem)
            .ok_or_else(|| format!("no executable in {}/MacOS in the bundle", contents.display()));
    };
    if inner.is_file() { Ok(inner) } else { Err(format!("no {} in the bundle", inner.display())) }
}

/// The executable in a macOS bundle's `MacOS` folder: the one named after the bundle without
/// its extension, as most bundles have, or else the first non-hidden file (the bundle format
/// allows any name; a hidden one, such as a stray `.DS_Store`, is not an executable).
pub(crate) fn macos_binary(macos: &Path, stem: &OsStr) -> Option<PathBuf> {
    let named = macos.join(stem);
    if named.is_file() {
        return Some(named);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(macos)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')))
        .collect();
    files.sort();
    files.into_iter().next()
}

impl ModuleFile {
    fn open(path: &Path) -> Result<Self, String> {
        let file = binary(path)?;
        // SAFETY: loading the library runs its initialisation code, which Rust cannot check;
        // that is what this process is for (ADR 0006). The library stays loaded while `self`
        // holds it, which outlives every object the factory makes.
        let library =
            unsafe { libloading::Library::new(&file) }.map_err(|e| format!("could not load the plug-in file: {e}"))?;
        #[cfg(windows)]
        {
            // SAFETY: `InitDll` is the optional entry the VST3 module API defines on Windows,
            // taking nothing and returning a bool.
            let init = unsafe { library.get::<extern "system" fn() -> bool>(b"InitDll\0") };
            if let Ok(init) = init
                && !init()
            {
                return Err("the plug-in's InitDll failed".into());
            }
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: `ModuleEntry` is the entry the VST3 module API defines on Linux; it takes
            // the library handle, which hosts may pass as null when they have none of their own.
            let entry = unsafe { library.get::<extern "system" fn(*mut c_void) -> bool>(b"ModuleEntry\0") };
            if let Ok(entry) = entry
                && !entry(std::ptr::null_mut())
            {
                return Err("the plug-in's ModuleEntry failed".into());
            }
        }
        #[cfg(target_os = "macos")]
        {
            // SAFETY: `bundleEntry` is the entry the VST3 module API defines on macOS, the
            // counterpart of `InitDll` on Windows and `ModuleEntry` on Linux: hosts that load
            // the module as a bundle call it before the factory. It is optional: modules built
            // on the current SDK initialise in their own static constructors and do not export
            // it. Those that do (DPF-built modules such as Cardinal and plugdata) build the
            // plugin their class information is read from here, and answering class queries
            // before it runs crashes them. The old contract passes the bundle's handle; these
            // modules find their own bundle from the loaded file, so null is fine.
            let entry = unsafe { library.get::<extern "system" fn(*mut c_void) -> bool>(b"bundleEntry\0") };
            if let Ok(entry) = entry
                && !entry(std::ptr::null_mut())
            {
                return Err("the plug-in's bundleEntry failed".into());
            }
        }
        // SAFETY: `GetPluginFactory` is the module's required entry: no arguments, returning an
        // owned reference to its factory.
        let get = unsafe { library.get::<extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0") }
            .map_err(|_| "the file has no GetPluginFactory; not a VST3 plug-in")?;
        // SAFETY: the pointer is the factory reference the module handed over (or null).
        let factory = unsafe { ComPtr::from_raw(get()) }.ok_or("the plug-in has no factory")?;
        Ok(Self { factory: Some(factory), library: Some(library) })
    }

    fn test(mix: bool) -> Self {
        Self { factory: crate::test_vst3::factory(mix), library: None }
    }

    fn factory(&self) -> Result<&ComPtr<IPluginFactory>, String> {
        self.factory.as_ref().ok_or_else(|| "the plug-in has no factory".into())
    }

    /// The audio classes: class id, name, and (from the newer class info) vendor, version and
    /// sub-categories.
    fn audio_classes(&self) -> Result<Vec<(TUID, PluginInfo)>, String> {
        let factory = self.factory()?;
        let mut factory_info: PFactoryInfo = blank();
        // SAFETY: the factory is a live COM object; the info is a plain struct it fills.
        let _ = unsafe { factory.getFactoryInfo(&mut factory_info) };
        let factory2 = factory.cast::<IPluginFactory2>();
        // SAFETY: as above.
        let count = unsafe { factory.countClasses() };
        let mut classes = Vec::new();
        for index in 0..count.clamp(0, 4096) {
            let mut info: PClassInfo = blank();
            // SAFETY: as above; the index is below the count the factory gave.
            if !ok(unsafe { factory.getClassInfo(index, &mut info) }) || text(&info.category) != AUDIO_CLASS {
                continue;
            }
            let mut plugin = PluginInfo {
                path: PathBuf::new(),
                id: cid_hex(&info.cid),
                name: text(&info.name),
                vendor: text(&factory_info.vendor),
                version: String::new(),
                features: Vec::new(),
                replaces: Vec::new(),
                ara: false,
            };
            if let Some(factory2) = &factory2 {
                let mut info2: PClassInfo2 = blank();
                // SAFETY: as above, through the newer interface the factory said it has.
                if ok(unsafe { factory2.getClassInfo2(index, &mut info2) }) {
                    if !text(&info2.vendor).is_empty() {
                        plugin.vendor = text(&info2.vendor);
                    }
                    plugin.version = text(&info2.version);
                    plugin.features =
                        text(&info2.subCategories).split('|').filter(|s| !s.is_empty()).map(str::to_string).collect();
                }
            }
            classes.push((info.cid, plugin));
        }
        Ok(classes)
    }

    /// The names of the module's ARA factory classes ([`suhina_plugins::scan::ARA_FACTORY_CLASS`]),
    /// read from the class list without making any object.
    fn ara_classes(&self) -> Vec<String> {
        let Ok(factory) = self.factory() else { return Vec::new() };
        // SAFETY: the factory is a live COM object.
        let count = unsafe { factory.countClasses() };
        (0..count.clamp(0, 4096))
            .filter_map(|index| {
                let mut info: PClassInfo = blank();
                // SAFETY: as above; the index is below the count the factory gave.
                let found = ok(unsafe { factory.getClassInfo(index, &mut info) });
                (found && text(&info.category) == suhina_plugins::scan::ARA_FACTORY_CLASS).then(|| text(&info.name))
            })
            .collect()
    }

    /// The module's compatibility list, from its `Plugin Compatibility Class`
    /// (`IPluginCompatibility`), as the JSON text it writes; `None` when it has none.
    fn compatibility(&self) -> Option<String> {
        let factory = self.factory().ok()?;
        // SAFETY: the factory is a live COM object.
        let count = unsafe { factory.countClasses() };
        let cid = (0..count.clamp(0, 4096)).find_map(|index| {
            let mut info: PClassInfo = blank();
            // SAFETY: as above; the index is below the count the factory gave.
            let found = ok(unsafe { factory.getClassInfo(index, &mut info) });
            (found && text(&info.category) == COMPATIBILITY_CLASS).then_some(info.cid)
        })?;
        let compatibility = self.create::<IPluginCompatibility>(&cid)?;
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>()?;
        // SAFETY: a live object writing to a live stream.
        if !ok(unsafe { compatibility.getCompatibilityJSON(raw.as_ptr()) }) {
            return None;
        }
        let mut bytes = stream.bytes();
        bytes.truncate(MAX_COMPATIBILITY);
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Creates an object of class `cid` and asks it for interface `I`.
    fn create<I: Interface>(&self, cid: &TUID) -> Option<ComPtr<I>> {
        let factory = self.factory().ok()?;
        let mut object: *mut c_void = std::ptr::null_mut();
        // SAFETY: both ids are 16-byte ids that outlive the call; the factory writes an owned
        // reference to `object` or leaves it null.
        let result = unsafe { factory.createInstance(cid.as_ptr(), I::IID.as_ptr().cast(), &mut object) };
        if !ok(result) {
            return None;
        }
        // SAFETY: on success `object` is an owned reference to the interface asked for.
        unsafe { ComPtr::from_raw(object.cast::<I>()) }
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        // Every object from the factory is gone by now; release the factory, then let the
        // module clean up, then unload it.
        self.factory = None;
        if let Some(library) = &self.library {
            #[cfg(windows)]
            {
                // SAFETY: `ExitDll` is the optional counterpart of `InitDll`.
                if let Ok(exit) = unsafe { library.get::<extern "system" fn() -> bool>(b"ExitDll\0") } {
                    exit();
                }
            }
            #[cfg(target_os = "linux")]
            {
                // SAFETY: `ModuleExit` is the counterpart of `ModuleEntry`.
                if let Ok(exit) = unsafe { library.get::<extern "system" fn() -> bool>(b"ModuleExit\0") } {
                    exit();
                }
            }
            #[cfg(target_os = "macos")]
            {
                // SAFETY: `bundleExit` is the counterpart of `bundleEntry`, optional the same
                // way: modules built on the current SDK do not export it.
                if let Ok(exit) = unsafe { library.get::<extern "system" fn() -> bool>(b"bundleExit\0") } {
                    exit();
                }
            }
            #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
            let _ = library;
        }
        self.library = None;
    }
}

/// A zeroed C struct for the plug-in to fill.
fn blank<T: Copy>() -> T {
    // SAFETY: only used for the SDK's plain C structs (ids, fixed-size strings, numbers), for
    // which all zero bytes is a valid value.
    unsafe { std::mem::zeroed() }
}

/// A module shared by every instance made from it in this process: the file is loaded, and
/// its `InitDll`/`ModuleEntry` runs, once however many instances use it, as the module API
/// asks.
type Module = Rc<ModuleFile>;

thread_local! {
    /// The modules loaded, by their binary's path (or `test:vst3`). Once loaded a module stays
    /// for the process's life and is never unloaded, its `ExitDll` never run: some plug-ins
    /// wait in it for threads that are still starting (Melodyne, let go of a second after it
    /// loaded to be loaded again as ARA, held the host for minutes), and the same plug-in is
    /// often loaded again soon (a device moved, a plug-in found to edit clips). The process
    /// ending frees them. Plug-ins are loaded on the host's main thread only.
    static MODULES: RefCell<HashMap<PathBuf, std::mem::ManuallyDrop<Module>>> = RefCell::new(HashMap::new());
}

/// The module loaded from `key`, or a new one `open` loads (and keeps for the next asker).
fn shared_module(key: PathBuf, open: impl FnOnce() -> Result<ModuleFile, String>) -> Result<Module, String> {
    let cached = MODULES.with(|modules| {
        let modules = modules.try_borrow().ok()?;
        modules.get(&key).map(|module| Module::clone(module))
    });
    if let Some(module) = cached {
        return Ok(module);
    }
    let module = Rc::new(open()?);
    MODULES.with(|modules| {
        if let Ok(mut modules) = modules.try_borrow_mut() {
            modules.insert(key, std::mem::ManuallyDrop::new(module.clone()));
        }
    });
    Ok(module)
}

/// The module and class a plug-in name points at.
fn open(spec: &str) -> Result<(Module, Option<TUID>), String> {
    if spec == "test:vst3" || spec == "test:vst3-mix" {
        let mix = spec == "test:vst3-mix";
        return Ok((shared_module(PathBuf::from(spec), || Ok(ModuleFile::test(mix)))?, None));
    }
    let file = spec.strip_prefix("vst3:").ok_or_else(|| format!("not a VST3 plug-in: {spec:?}"))?;
    let (path, cid) = match file.rsplit_once('#') {
        Some((path, hex)) => (path, Some(cid_from_hex(hex).ok_or_else(|| format!("not a class id: {hex:?}"))?)),
        None => (file, None),
    };
    let path = Path::new(path);
    let key = binary(path)?;
    let key = key.canonicalize().unwrap_or(key);
    Ok((shared_module(key, || ModuleFile::open(path))?, cid))
}

/// The plug-ins the file `spec` names holds, without making any (for scanning).
pub(crate) fn describe(spec: &str) -> Result<Vec<PluginInfo>, String> {
    let (module, _) = open(spec)?;
    let mut plugins: Vec<PluginInfo> = module.audio_classes()?.into_iter().map(|(_, info)| info).collect();
    if let Some(list) = module.compatibility() {
        suhina_plugins::scan::apply_compatibility_text(&mut plugins, &list);
    }
    suhina_plugins::scan::pair_ara(&mut plugins, &module.ara_classes());
    Ok(plugins)
}

// ---- What the host gives the plug-in -------------------------------------------------------

struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        // SAFETY: the plug-in passes a pointer to a String128 of its own to fill.
        let Some(name) = (unsafe { name.as_mut() }) else { return kInvalidArgument };
        name.fill(0);
        for (slot, unit) in name.iter_mut().zip("Suhina".encode_utf16()) {
            *slot = unit as TChar;
        }
        kResultOk
    }

    /// Makes a message or an attribute list for the plug-in (`crate::vst3_message`), as the
    /// SDK's hosts do; component and controller talk with them, and many plug-ins crash when
    /// the host makes none.
    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        // SAFETY: the plug-in passes a pointer to its own pointer to fill (or null).
        let Some(obj) = (unsafe { obj.as_mut() }) else { return kInvalidArgument };
        *obj = crate::vst3_message::create(cid, iid);
        if obj.is_null() { kResultFalse } else { kResultOk }
    }
}

/// What the controller tells the host: the user moving a control in the editor. Each edit goes
/// to the processor with its next block, as the SDK asks the host to do.
struct ComponentHandler {
    pending: Pending,
    /// The `restartComponent` flags the plug-in asked with, not yet served (from any thread).
    restart: Arc<AtomicI32>,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if !value.is_finite() {
            return kInvalidArgument;
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|(other, _)| *other != id);
            pending.push((id, value.clamp(0.0, 1.0)));
        }
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart.fetch_or(flags, Ordering::AcqRel);
        kResultOk
    }
}

/// A byte stream in memory, for plug-in state.
pub(crate) struct MemoryStream {
    data: Mutex<(Vec<u8>, usize)>,
}

impl MemoryStream {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self { data: Mutex::new((bytes, 0)) }
    }

    pub(crate) fn rewind(&self) {
        if let Ok(mut data) = self.data.lock() {
            data.1 = 0;
        }
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.data.lock().map(|d| d.0.clone()).unwrap_or_default()
    }
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, bytes: int32, read: *mut int32) -> tresult {
        let Ok(mut data) = self.data.lock() else { return kResultFalse };
        let (bytes_now, at) = (&data.0, data.1);
        let count = usize::try_from(bytes).unwrap_or(0).min(bytes_now.len().saturating_sub(at));
        if count > 0 && !buffer.is_null() {
            let from = bytes_now.as_ptr().wrapping_add(at);
            // SAFETY: the plug-in's buffer holds at least `bytes` bytes, and `count` is no
            // more than that or than what is left of ours; they are separate allocations.
            unsafe { std::ptr::copy_nonoverlapping(from, buffer.cast::<u8>(), count) };
        }
        data.1 = at + count;
        // SAFETY: the plug-in passes a pointer to a count of its own to fill (or null).
        if let Some(read) = unsafe { read.as_mut() } {
            *read = count as int32;
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, bytes: int32, written: *mut int32) -> tresult {
        let Ok(mut data) = self.data.lock() else { return kResultFalse };
        let count = usize::try_from(bytes).unwrap_or(0);
        if count > 0 && !buffer.is_null() {
            // SAFETY: the plug-in's buffer holds `bytes` bytes for the length of the call.
            let incoming = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), count) };
            let at = data.1;
            let end = at + count;
            if data.0.len() < end {
                data.0.resize(end, 0);
            }
            if let Some(target) = data.0.get_mut(at..end) {
                target.copy_from_slice(incoming);
            }
            data.1 = end;
        }
        // SAFETY: as in `read`.
        if let Some(written) = unsafe { written.as_mut() } {
            *written = count as int32;
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut data) = self.data.lock() else { return kResultFalse };
        let base = match mode as IBStream_::IStreamSeekMode {
            IBStream_::IStreamSeekMode_::kIBSeekSet => 0,
            IBStream_::IStreamSeekMode_::kIBSeekCur => data.1 as int64,
            IBStream_::IStreamSeekMode_::kIBSeekEnd => data.0.len() as int64,
            _ => return kInvalidArgument,
        };
        let Some(target) = base.checked_add(pos).filter(|t| *t >= 0) else { return kInvalidArgument };
        data.1 = usize::try_from(target).unwrap_or(usize::MAX).min(data.0.len());
        // SAFETY: as in `read`.
        if let Some(result) = unsafe { result.as_mut() } {
            *result = data.1 as int64;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(data) = self.data.lock() else { return kResultFalse };
        // SAFETY: as in `read`.
        match unsafe { pos.as_mut() } {
            Some(pos) => {
                *pos = data.1 as int64;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

// ---- A running plug-in --------------------------------------------------------------------

/// The audio processor, handed to the audio thread.
struct AudioSide(ComPtr<IAudioProcessor>);

// SAFETY: the VST3 threading model has the audio processor's `setProcessing` and `process`
// called from the audio thread while the component is used from the main thread; this pointer
// is only used for those two calls, on one thread at a time.
unsafe impl Send for AudioSide {}

/// A loaded, active VST3 plug-in and its audio thread.
pub(crate) struct Vst3Plugin {
    /// The audio thread; it hands the link back when it stops.
    audio: Option<thread::JoinHandle<Served>>,
    stop: Arc<AtomicBool>,
    /// The audio thread is stopping for a restart: it hands the processor the parameter values
    /// still waiting first.
    flush: Arc<AtomicBool>,
    latency: u32,
    /// The `restartComponent` flags the plug-in asked with.
    restart: Arc<AtomicI32>,
    /// What the audio thread is started with: each bus's channels (inputs, outputs) and the
    /// sample rate.
    buses: (Vec<usize>, Vec<usize>),
    sample_rate: f64,
    /// Its number in the process, which its audio thread serves as.
    instance: u32,
    /// What its audio thread played since the last trace line; written by that thread, traced
    /// from [`Vst3Plugin::tick`] (`docs/debugging.md`).
    watch: Arc<Watch>,
    connections: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    /// Parameter values for the processor's next block.
    pending: Pending,
    /// What the editor changed and the audio thread took in, for [`Vst3Plugin::tick`] to give
    /// the controller and report to the session.
    applied: Notes,
    /// What the engine sent with a block (a panel set, automation): the last value per
    /// parameter, for [`Vst3Plugin::tick`] to give the controller so its editor follows.
    engine: Notes,
    /// What each report's slots said at the last [`Vst3Plugin::tick`], so a value is reported
    /// once ([`Reads`]).
    reads: (Reads, Reads),
    /// The values the audio thread gave mapped MIDI controllers, given to the controller too
    /// on the main thread ([`Vst3Plugin::tick`]) so the editor shows them.
    shown: Option<Shown>,
    /// With a trace, what the audio thread gave the plug-in, written here on the main thread
    /// ([`Vst3Plugin::tick`]) so the audio thread never formats or writes.
    midi_watch: Option<Arc<MidiWatch>>,
    /// The plug-in's name, for its editor's window.
    name: String,
    editor: Option<crate::vst3_editor::Editor>,
    /// The ARA document it is bound to, when it was loaded with one (ADR 0028); let go of
    /// before the component is terminated.
    ara: Option<Box<crate::ara::Document>>,
    /// Its component answers ARA's entry point, so it can be bound to a document.
    supports_ara: bool,
    component: ComPtr<IComponent>,
    _handler: ComWrapper<ComponentHandler>,
    _host: ComWrapper<HostApplication>,
    /// When it was loaded: let go of soon after, it is released later (`Loaded::unload`).
    loaded_at: std::time::Instant,
    module: Module,
}

/// Channels in a speaker arrangement (one bit each).
fn channel_count(arrangement: SpeakerArrangement) -> usize {
    arrangement.count_ones() as usize
}

impl Vst3Plugin {
    /// Loads the plug-in `spec` names, activates it for the link's block size at `sample_rate`
    /// and starts serving blocks from `served` on an audio thread, as instance `instance` of
    /// the process.
    pub(crate) fn load(
        spec: &str,
        instance: u32,
        served: Served,
        sample_rate: f64,
        ara: Option<suhina_plugins::protocol::AraDocument>,
    ) -> Result<Self, String> {
        let (module, wanted) = open(spec)?;
        let classes = module.audio_classes()?;
        let (cid, name) = match wanted {
            Some(wanted) => classes.iter().find(|(cid, _)| *cid == wanted),
            None => classes.first(),
        }
        .map(|(cid, info)| (*cid, info.name.clone()))
        .ok_or_else(|| format!("no audio plug-in in {spec:?}"))?;

        let component = module.create::<IComponent>(&cid).ok_or("could not make the plug-in's component")?;
        let host = ComWrapper::new(HostApplication);
        let pending = Pending::default();
        let applied = Notes::new(ParamNotes::new());
        let engine = Notes::new(ParamNotes::new());
        let reads = (ParamNotes::fresh_reads(), ParamNotes::fresh_reads());
        let restart = Arc::new(AtomicI32::new(0));
        let handler = ComWrapper::new(ComponentHandler { pending: pending.clone(), restart: restart.clone() });
        let context = host.to_com_ptr::<FUnknown>().ok_or("host object")?;
        // SAFETY: a live component, given the host's context object for the call.
        if !ok(unsafe { component.initialize(context.as_ptr()) }) {
            return Err("the plug-in's component did not initialise".into());
        }

        // The controller: the component itself, or a class of its own.
        let (controller, separate_controller) = match component.cast::<IEditController>() {
            Some(controller) => (Some(controller), false),
            None => {
                let mut controller_cid: TUID = [0; 16];
                // SAFETY: a live component; the id is a 16-byte buffer to fill.
                let has = ok(unsafe { component.getControllerClassId(&mut controller_cid) });
                let controller = if has { module.create::<IEditController>(&controller_cid) } else { None };
                // SAFETY: a live controller, given the host's context object for the call.
                let controller = controller.filter(|c| ok(unsafe { c.initialize(context.as_ptr()) }));
                (controller, true)
            }
        };
        let mut plugin = Self {
            audio: None,
            stop: Arc::new(AtomicBool::new(false)),
            flush: Arc::new(AtomicBool::new(false)),
            latency: 0,
            restart,
            buses: (Vec::new(), Vec::new()),
            sample_rate,
            instance,
            connections: None,
            controller,
            separate_controller,
            pending,
            applied,
            engine,
            reads,
            shown: None,
            midi_watch: None,
            name,
            editor: None,
            ara: None,
            supports_ara: false,
            component,
            _handler: handler,
            _host: host,
            loaded_at: std::time::Instant::now(),
            watch: Arc::new(Watch::default()),
            module,
        };
        plugin.connect();
        plugin.sync_controller();
        if let (Some(controller), Some(handler)) =
            (&plugin.controller, plugin._handler.to_com_ptr::<IComponentHandler>())
        {
            // SAFETY: a live controller and a handler that outlives it (it is let go of in
            // `drop` before the controller is terminated).
            let _ = unsafe { controller.setComponentHandler(handler.as_ptr()) };
        }

        // ARA binds before processing is set up (the ARA SDK's order).
        if let Some(document) = &ara {
            plugin.ara = Some(Box::new(crate::ara::Document::bind(&plugin.component, document)?));
        }
        plugin.supports_ara = plugin.ara.is_some() || crate::ara::supported(&plugin.component);

        let processor = plugin.component.cast::<IAudioProcessor>().ok_or("the plug-in does not process audio")?;
        let max_frames = served.max_frames();
        plugin.buses = plugin.set_up_buses(&processor);
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: max_frames as int32,
            sampleRate: sample_rate,
        };
        // SAFETY: a live processor; the setup struct lives for the call.
        if !ok(unsafe { processor.setupProcessing(&mut setup) }) {
            return Err("the plug-in refused the sample rate or block size".into());
        }
        // SAFETY: a live, set-up component.
        if !ok(unsafe { plugin.component.setActive(1) }) {
            return Err("the plug-in did not activate".into());
        }
        // SAFETY: a live, active processor.
        plugin.latency = unsafe { processor.getLatencySamples() };
        // What it asked while being set up is served by the setting up.
        plugin.restart.store(0, Ordering::Release);
        plugin.start_audio(processor, served)?;
        Ok(plugin)
    }

    /// Starts an audio thread that serves blocks from `served` through the active `processor`.
    fn start_audio(&mut self, processor: ComPtr<IAudioProcessor>, served: Served) -> Result<(), String> {
        self.stop = Arc::new(AtomicBool::new(false));
        self.flush = Arc::new(AtomicBool::new(false));
        let (stop, flush) = (self.stop.clone(), self.flush.clone());
        let (pending, applied, engine) = (self.pending.clone(), self.applied.clone(), self.engine.clone());
        let mapping = MidiMapping::of(self.controller.as_ref());
        self.shown = Some(mapping.shown());
        self.midi_watch = suhina_plugins::trace::on().then(|| Arc::new(MidiWatch::default()));
        let midi_watch = self.midi_watch.clone();
        let (buses, sample_rate, instance) = (self.buses.clone(), self.sample_rate, self.instance);
        let watch = (self.ara.is_some() && suhina_plugins::trace::on()).then(|| self.watch.clone());
        let audio = AudioSide(processor);
        let thread = thread::Builder::new()
            .name("plug-in audio".into())
            .spawn(move || {
                let _realtime = crate::real_time_thread();
                serve(
                    instance,
                    audio,
                    served,
                    buses,
                    sample_rate,
                    (pending, applied, engine, mapping, midi_watch),
                    (&stop, &flush),
                    watch,
                )
            })
            .map_err(|e| e.to_string())?;
        self.audio = Some(thread);
        Ok(())
    }

    /// A latency the plug-in reported since this was last asked: one that asked for a restart
    /// because its latency changed is deactivated and activated again first, as the SDK asks.
    pub(crate) fn latency_changed(&mut self) -> Option<u32> {
        let flags = self.restart.swap(0, Ordering::AcqRel);
        if flags & RestartFlags_::kLatencyChanged == 0 {
            return None;
        }
        match self.reactivate() {
            Ok(latency) if latency != self.latency => {
                self.latency = latency;
                Some(latency)
            }
            Ok(_) => None,
            Err(why) => {
                eprintln!("plug-in {}: could not restart it: {why}", self.name);
                None
            }
        }
    }

    /// Stops the audio thread, deactivates and activates the component, and starts a new audio
    /// thread on the same link; returns the latency it reports then. An audio thread stuck in
    /// the plug-in is left as it is, and so is the plug-in.
    fn reactivate(&mut self) -> Result<u32, String> {
        self.reactivate_at(None)
    }

    pub(crate) fn reconfigure(&mut self, sample_rate: f64) -> Result<u32, String> {
        if self.sample_rate == sample_rate {
            return Ok(self.latency);
        }
        let latency = self.reactivate_at(Some(sample_rate))?;
        self.latency = latency;
        Ok(latency)
    }

    fn reactivate_at(&mut self, sample_rate: Option<f64>) -> Result<u32, String> {
        self.flush.store(true, Ordering::Release);
        if !self.stop_audio(crate::UNLOAD_WAIT) {
            return Err("its audio thread does not stop".into());
        }
        let served =
            self.audio.take().ok_or("it has no audio thread")?.join().map_err(|_| "its audio thread failed")?;
        let processor = self.component.cast::<IAudioProcessor>().ok_or("the plug-in does not process audio")?;
        // SAFETY: a live component; processing has stopped.
        let _ = unsafe { self.component.setActive(0) };
        if let Some(sample_rate) = sample_rate {
            let mut setup = ProcessSetup {
                processMode: ProcessModes_::kRealtime as int32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
                maxSamplesPerBlock: served.max_frames() as int32,
                sampleRate: sample_rate,
            };
            // SAFETY: the audio thread has joined and the live component is inactive;
            // setup is borrowed only for this main-thread call.
            if !ok(unsafe { processor.setupProcessing(&mut setup) }) {
                return Err("the plug-in refused the new audio configuration".into());
            }
            self.sample_rate = sample_rate;
        }
        // SAFETY: as above, set up as when it loaded.
        if !ok(unsafe { self.component.setActive(1) }) {
            return Err("the plug-in did not activate again".into());
        }
        // SAFETY: a live, active processor.
        let latency = unsafe { processor.getLatencySamples() };
        self.start_audio(processor, served)?;
        Ok(latency)
    }

    /// Connects component and controller, when they are separate objects that talk.
    fn connect(&mut self) {
        if !self.separate_controller {
            return;
        }
        let Some(controller) = &self.controller else { return };
        let (Some(a), Some(b)) = (self.component.cast::<IConnectionPoint>(), controller.cast::<IConnectionPoint>())
        else {
            return;
        };
        // SAFETY: two live connection points, each given the other.
        let _ = unsafe { a.connect(b.as_ptr()) };
        // SAFETY: as above, the other way.
        let _ = unsafe { b.connect(a.as_ptr()) };
        self.connections = Some((a, b));
    }

    /// Hands the component's state to the controller, so both agree.
    fn sync_controller(&self) {
        let Some(controller) = &self.controller else { return };
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let Some(raw) = stream.to_com_ptr::<IBStream>() else { return };
        // SAFETY: a live component writing to a live stream.
        if ok(unsafe { self.component.getState(raw.as_ptr()) }) {
            stream.rewind();
            // SAFETY: a live controller reading the same stream.
            let _ = unsafe { controller.setComponentState(raw.as_ptr()) };
        }
    }

    /// Asks for stereo on the main buses, activates them, and returns each bus's channel count
    /// (inputs, outputs).
    fn set_up_buses(&self, processor: &ComPtr<IAudioProcessor>) -> (Vec<usize>, Vec<usize>) {
        let audio = MediaTypes_::kAudio as MediaType;
        let count = |direction: BusDirections| -> i32 {
            // SAFETY: a live component.
            unsafe { self.component.getBusCount(audio, direction as BusDirection) }.clamp(0, 64)
        };
        let (inputs, outputs) = (count(BusDirections_::kInput), count(BusDirections_::kOutput));
        let arrangement = |direction: BusDirections, index: i32| -> SpeakerArrangement {
            let mut arrangement: SpeakerArrangement = 0;
            // SAFETY: a live processor writing one value.
            let _ = unsafe { processor.getBusArrangement(direction as BusDirection, index, &mut arrangement) };
            arrangement
        };
        let mut ins: Vec<SpeakerArrangement> = (0..inputs).map(|i| arrangement(BusDirections_::kInput, i)).collect();
        let mut outs: Vec<SpeakerArrangement> = (0..outputs).map(|i| arrangement(BusDirections_::kOutput, i)).collect();
        if let Some(main) = ins.first_mut() {
            *main = SpeakerArr::kStereo;
        }
        if let Some(main) = outs.first_mut() {
            *main = SpeakerArr::kStereo;
        }
        // SAFETY: a live processor; both arrays hold as many entries as the counts given.
        let _ = unsafe { processor.setBusArrangements(ins.as_mut_ptr(), inputs, outs.as_mut_ptr(), outputs) };
        let in_channels: Vec<usize> =
            (0..inputs).map(|i| channel_count(arrangement(BusDirections_::kInput, i))).collect();
        let out_channels: Vec<usize> =
            (0..outputs).map(|i| channel_count(arrangement(BusDirections_::kOutput, i))).collect();
        for (direction, buses) in [(BusDirections_::kInput, inputs), (BusDirections_::kOutput, outputs)] {
            if buses > 0 {
                // SAFETY: a live component; bus 0 exists.
                let _ = unsafe { self.component.activateBus(audio, direction as BusDirection, 0, 1) };
            }
        }
        // An instrument takes notes on its first event input.
        let event = MediaTypes_::kEvent as MediaType;
        let input = BusDirections_::kInput as BusDirection;
        // SAFETY: a live component.
        if unsafe { self.component.getBusCount(event, input) } > 0 {
            // SAFETY: as above; event bus 0 exists.
            let _ = unsafe { self.component.activateBus(event, input, 0, 1) };
        }
        (in_channels, out_channels)
    }

    /// Leaves its ARA document now, with its processing stopped: an instance released later
    /// (`Loaded::unload`) must not keep its track's regions in the document the other tracks
    /// share, where the instance loaded in its place puts them again.
    pub(crate) fn leave_document(&mut self) {
        if self.ara.is_none() {
            return;
        }
        self.stop.store(true, Ordering::Release);
        if let Some(audio) = self.audio.take() {
            let _ = audio.join();
        }
        // SAFETY: a live component; processing has stopped.
        let _ = unsafe { self.component.setActive(0) };
        self.ara = None;
    }

    /// How long ago it was loaded.
    pub(crate) fn age(&self) -> std::time::Duration {
        self.loaded_at.elapsed()
    }

    /// Asks the audio thread to stop and waits up to `wait` for it; whether it stopped.
    pub(crate) fn stop_audio(&mut self, wait: std::time::Duration) -> bool {
        crate::audio_stopped(&self.stop, self.audio.as_ref(), wait)
    }

    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    pub(crate) fn state(&mut self) -> Result<Vec<u8>, String> {
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>().ok_or("stream")?;
        // SAFETY: a live component writing to a live stream.
        if !ok(unsafe { self.component.getState(raw.as_ptr()) }) {
            return Err("the plug-in did not give its state".into());
        }
        // Bound to an ARA document, its edits there are part of its state (ADR 0028).
        Ok(crate::ara::wrap_state(stream.bytes(), self.ara.as_deref().and_then(crate::ara::Document::store)))
    }

    /// The controller's own state, when it is a separate object that gives one.
    pub(crate) fn controller_state(&mut self) -> Option<Vec<u8>> {
        let controller = self.controller.as_ref()?;
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>()?;
        // SAFETY: a live controller writing to a live stream.
        ok(unsafe { controller.getState(raw.as_ptr()) }).then(|| stream.bytes())
    }

    /// Hands `data` to the component and mirrors it to the controller, then gives the
    /// controller its own `controller` state, as a project saved it. A VST3 plug-in that stands
    /// in for a VST2 one reads the VST2 bank here (ADR 0006, decision 8); a bank of programs
    /// is also set as parameters, since not every plug-in reads one.
    pub(crate) fn set_state(&mut self, data: &[u8], controller_state: Option<&[u8]>) -> Result<(), String> {
        let (data, archive) = crate::ara::unwrap_state(data);
        let stream = ComWrapper::new(MemoryStream::new(data.to_vec()));
        let raw = stream.to_com_ptr::<IBStream>().ok_or("stream")?;
        // SAFETY: a live component reading from a live stream.
        let taken = ok(unsafe { self.component.setState(raw.as_ptr()) });
        let parameters = suhina_plugins::vst2::program_values(data).map_or(0, |values| self.set_parameters(&values));
        if !taken && parameters == 0 {
            return Err("the plug-in did not take the state".into());
        }
        if let Some(controller) = &self.controller {
            if taken {
                stream.rewind();
                // SAFETY: a live controller reading the same stream.
                let _ = unsafe { controller.setComponentState(raw.as_ptr()) };
            }
            if let Some(own) = controller_state.filter(|s| !s.is_empty()) {
                let own = ComWrapper::new(MemoryStream::new(own.to_vec()));
                let own_raw = own.to_com_ptr::<IBStream>().ok_or("stream")?;
                // SAFETY: a live controller reading a live stream. A controller that refuses
                // its own state still has the component's, so this is not an error.
                let _ = unsafe { controller.setState(own_raw.as_ptr()) };
            }
        }
        // Its ARA edits go back to the clips they were made on; without a document (the track
        // has no audio clips now) they wait in the project for one.
        if let (Some(document), Some((id, bytes))) = (&self.ara, archive) {
            document.restore(&id, bytes)?;
        }
        Ok(())
    }

    /// Opens the plug-in's editor as `mode` says (or returns the size of the one open).
    pub(crate) fn open_editor(&mut self, mode: crate::window::Mode) -> Result<crate::window::Size, String> {
        if let Some(editor) = &self.editor {
            return Ok(editor.size());
        }
        let controller = self.controller.as_ref().ok_or("the plug-in has no editor")?;
        let editor = crate::vst3_editor::Editor::open(controller, mode, &self.name)?;
        let size = editor.size();
        self.editor = Some(editor);
        Ok(size)
    }

    /// Whether it supports ARA: it is bound to a document, or its component answers
    /// `IPlugInEntryPoint` (found when it loaded).
    pub(crate) fn edits_clips(&self) -> bool {
        self.ara.is_some() || self.supports_ara
    }

    /// Between messages: lets a bound ARA document send its queued model changes (#718), gives
    /// the controller what MIDI controllers moved, so the editor shows it (#812), what the
    /// editor itself changed and what the engine sent with the blocks (a panel set, an
    /// automation lane), so the plug-in's editor and its parameter listing follow them both.
    /// Returns what the editor changed: (id, value) pairs the session keeps the project's
    /// values in step with.
    pub(crate) fn tick(&mut self) -> Vec<(ParamID, ParamValue)> {
        if let Some(ara) = &self.ara {
            ara.tick();
            self.trace_output();
        }
        // Take both reports out at once; the controller updates below run with no report
        // held (the audio thread never locks at all, so its next block never waits on one).
        let mut made_in_the_editor = Vec::new();
        for (taken, to_session) in
            [(self.applied.take(&mut self.reads.0), true), (self.engine.take(&mut self.reads.1), false)]
        {
            for (id, value) in taken {
                if let Some(controller) = &self.controller {
                    // The audio thread took it in; the controller's own cache keeps the value,
                    // which a parameter listing reads and the editor draws.
                    // SAFETY: a live controller, on the main thread, for a parameter it listed.
                    let _ = unsafe { controller.setParamNormalized(id, value) };
                }
                if to_session {
                    made_in_the_editor.push((id, value));
                }
            }
        }
        self.show_controllers();
        if let Some(watch) = &self.midi_watch {
            watch.write(self.instance);
        }
        made_in_the_editor
    }

    /// With a trace, once a second of audio: whether the ARA-bound plug-in plays, the song's
    /// place it was told and the loudest sample (a silent ARA track says here whether the
    /// plug-in played nothing or was not asked to). Written here, on the main thread, from what
    /// the audio thread set ([`Watch`]).
    fn trace_output(&self) {
        let rate = self.sample_rate;
        let watch = &self.watch;
        if rate <= 0.0 || (watch.frames.load(Ordering::Acquire) as f64) < rate {
            return;
        }
        watch.take_frames();
        let peak = watch.take_peak();
        let playing = watch.playing.load(Ordering::Relaxed);
        let track = self.ara.as_ref().map_or("", |ara| ara.track());
        suhina_plugins::trace!(
            "ara audio {} ({}): {}, song at {:.2} s, peak {:.4}",
            self.instance,
            track,
            if playing { "playing" } else { "stopped" },
            watch.song_samples.load(Ordering::Relaxed) as f64 / rate,
            peak
        );
    }

    /// How far the analysis of a bound ARA document's track has come, when it changed.
    pub(crate) fn take_ara_analysis(&self) -> Option<Option<f32>> {
        self.ara.as_ref().and_then(|ara| ara.take_analysis())
    }

    /// The notes a bound ARA document read since the last call (#720).
    pub(crate) fn take_ara_notes(&self) -> Vec<(String, Vec<suhina_plugins::protocol::AraNote>, bool)> {
        self.ara.as_ref().map(|ara| ara.take_notes()).unwrap_or_default()
    }

    /// The edits of one modification and its source, stored on their own (ADR 0028 step 6).
    pub(crate) fn ara_export(&self, modification: &str, source: &str) -> Result<(String, Vec<u8>), String> {
        self.ara.as_ref().ok_or("the plug-in is not bound to an ARA document")?.export(modification, source)
    }

    /// Gives the bound document edits another track's stored for one of its modifications.
    pub(crate) fn ara_import(&self, id: &str, bytes: &[u8], from: &str, to: &str, source: &str) -> Result<(), String> {
        self.ara.as_ref().ok_or("the plug-in is not bound to an ARA document")?.import(id, bytes, from, to, source)
    }

    pub(crate) fn ara_update(&mut self, document: &suhina_plugins::protocol::AraDocument) -> Result<(), String> {
        let edited = self.ara.as_mut().ok_or("the plug-in is not bound to an ARA document")?.edit(document)?;
        if edited.changes_renderers() {
            self.flush.store(true, Ordering::Release);
            if !self.stop_audio(crate::UNLOAD_WAIT) {
                return Err("its audio thread does not stop".into());
            }
            let served =
                self.audio.take().ok_or("it has no audio thread")?.join().map_err(|_| "its audio thread failed")?;
            let processor = self.component.cast::<IAudioProcessor>().ok_or("the plug-in does not process audio")?;
            // SAFETY: a live component; processing has stopped.
            let _ = unsafe { self.component.setActive(0) };
            if let Some(ara) = &self.ara {
                ara.renderers(&edited);
            }
            // SAFETY: as above, set up as when it loaded.
            let active = ok(unsafe { self.component.setActive(1) });
            self.start_audio(processor, served)?;
            if !active {
                return Err("the plug-in did not activate again".into());
            }
        }
        if let Some(ara) = self.ara.as_mut() {
            ara.destroy(&edited);
        }
        Ok(())
    }

    /// Tells the plug-in's ARA editor view which region the engine has selected, when it is
    /// bound to a document.
    pub(crate) fn ara_select(&self, region: Option<&str>) {
        if let Some(document) = &self.ara {
            document.select(region);
        }
    }

    /// Asks the open editor to take `size`, the room the engine has for it.
    pub(crate) fn fit_editor(&mut self, size: crate::window::Size) {
        if let Some(editor) = &mut self.editor {
            editor.fit(size);
        }
    }

    /// Puts an embedded editor where the engine says.
    pub(crate) fn place_editor(&self, placement: crate::window::Placement) {
        if let Some(editor) = &self.editor {
            editor.place(placement);
        }
    }

    pub(crate) fn close_editor(&mut self) {
        if let Some(editor) = self.editor.take() {
            editor.close();
        }
    }

    /// Keeps the editor in step with its window; returns whether the user closed it.
    pub(crate) fn editor_tick(&mut self) -> bool {
        let closed = self.editor.as_mut().is_some_and(crate::vst3_editor::Editor::tick);
        if closed {
            self.close_editor();
        }
        closed
    }

    /// A size the editor got since this was last asked.
    pub(crate) fn editor_resized(&mut self) -> Option<crate::window::Size> {
        self.editor.as_mut().and_then(crate::vst3_editor::Editor::take_resized)
    }

    /// What a MIDI controller moved goes to the controller as well, which the editor draws
    /// from: the processor alone has it otherwise. On the main thread.
    fn show_controllers(&self) {
        if let (Some(shown), Some(controller)) = (&self.shown, &self.controller) {
            for (id, value) in shown.take() {
                // SAFETY: a live controller, on the main thread, for a parameter its own
                // `IMidiMapping` named.
                let _ = unsafe { controller.setParamNormalized(id, value) };
            }
        }
    }

    /// The controller's parameters, values normalized (0 to 1). What MIDI controllers moved in
    /// the blocks played so far is handed to the controller first, so a read right after a block
    /// does not wait for the next turn of the host's loop (#887).
    pub(crate) fn params(&self) -> Vec<ParamInfo> {
        self.show_controllers();
        let Some(controller) = &self.controller else { return Vec::new() };
        // SAFETY: a live controller.
        let count = unsafe { controller.getParameterCount() }.clamp(0, 1 << 16);
        (0..count)
            .filter_map(|index| {
                let mut info: ParameterInfo = blank();
                // SAFETY: as above; the index is below the count it gave.
                ok(unsafe { controller.getParameterInfo(index, &mut info) }).then(|| self.param(controller, &info))
            })
            .collect()
    }

    /// Parameter `info` with its current value and text.
    fn param(&self, controller: &ComPtr<IEditController>, info: &ParameterInfo) -> ParamInfo {
        use ParameterInfo_::ParameterFlags_ as Flags;
        // SAFETY: a live controller, for a parameter it listed.
        let value = unsafe { controller.getParamNormalized(info.id) };
        let mut shown: String128 = [0; 128];
        // SAFETY: as above, writing at most 128 UTF-16 units.
        let text = if ok(unsafe { controller.getParamStringByValue(info.id, value, &mut shown) }) {
            utf16(&shown)
        } else {
            String::new()
        };
        let flag = |f: ParameterInfo_::ParameterFlags| info.flags & f as int32 != 0;
        ParamInfo {
            id: info.id,
            name: utf16(&info.title),
            group: String::new(),
            min: 0.0,
            max: 1.0,
            default: finite(info.defaultNormalizedValue, 0.0),
            value: finite(value, 0.0),
            text,
            steps: u32::try_from(info.stepCount).unwrap_or(0),
            automatable: flag(Flags::kCanAutomate),
            read_only: flag(Flags::kIsReadOnly),
            hidden: flag(Flags::kIsHidden),
        }
    }

    /// Sets parameter `id` to `value` (normalized; kept within 0 to 1): on the controller now,
    /// on the processor with its next block.
    pub(crate) fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        let controller = self.controller.as_ref().ok_or("the plug-in has no parameters")?;
        // SAFETY: a live controller.
        let count = unsafe { controller.getParameterCount() }.clamp(0, 1 << 16);
        let info = (0..count)
            .find_map(|index| {
                let mut info: ParameterInfo = blank();
                // SAFETY: as above; the index is below the count it gave.
                (ok(unsafe { controller.getParameterInfo(index, &mut info) }) && info.id == id).then_some(info)
            })
            .ok_or_else(|| format!("no parameter {id}"))?;
        let value = value.clamp(0.0, 1.0);
        // SAFETY: a live controller, for a parameter it listed.
        let _ = unsafe { controller.setParamNormalized(id, value) };
        if let Ok(mut pending) = self.pending.lock() {
            pending.push((id, value));
        }
        Ok(self.param(controller, &info))
    }

    /// Sets parameter `i` to `values[i]` (normalized), the numbering a VST3 version keeps from
    /// its VST2 one: on the controller now, on the processor with its next block. Only when the
    /// controller has every id from 0 to the last value's, so a plug-in that numbers its
    /// parameters another way is left alone. Returns how many were set.
    fn set_parameters(&mut self, values: &[f32]) -> usize {
        let Some(controller) = &self.controller else { return 0 };
        // SAFETY: a live controller.
        let count = unsafe { controller.getParameterCount() }.clamp(0, 1 << 16);
        let ids: std::collections::HashSet<ParamID> = (0..count)
            .filter_map(|index| {
                let mut info: ParameterInfo = blank();
                // SAFETY: as above; the index is below the count it gave.
                ok(unsafe { controller.getParameterInfo(index, &mut info) }).then_some(info.id)
            })
            .collect();
        let changes: Option<Vec<(ParamID, ParamValue)>> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let id = ParamID::try_from(index).ok().filter(|id| ids.contains(id))?;
                Some((id, f64::from(*value).clamp(0.0, 1.0)))
            })
            .collect();
        let Some(changes) = changes else { return 0 };
        for (id, value) in &changes {
            // SAFETY: as above, for a parameter it listed.
            let _ = unsafe { controller.setParamNormalized(*id, *value) };
        }
        let set = changes.len();
        if let Ok(mut pending) = self.pending.lock() {
            pending.extend(changes);
        }
        set
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        self.close_editor();
        self.stop.store(true, Ordering::Release);
        suhina_plugins::trace!("vst3 drop: joining the audio thread");
        if let Some(audio) = self.audio.take() {
            let _ = audio.join();
        }
        suhina_plugins::trace!("vst3 drop: setActive(0)");
        // SAFETY: a live component; processing has stopped.
        let _ = unsafe { self.component.setActive(0) };
        suhina_plugins::trace!("vst3 drop: ARA document");
        self.ara = None;
        suhina_plugins::trace!("vst3 drop: disconnect, controller");
        if let Some((a, b)) = self.connections.take() {
            // SAFETY: the two connection points connected in `connect`.
            let _ = unsafe { a.disconnect(b.as_ptr()) };
            // SAFETY: as above, the other way.
            let _ = unsafe { b.disconnect(a.as_ptr()) };
        }
        if let Some(controller) = &self.controller {
            // SAFETY: a live controller letting go of the handler before it is released.
            let _ = unsafe { controller.setComponentHandler(std::ptr::null_mut()) };
        }
        if self.separate_controller
            && let Some(controller) = self.controller.take()
        {
            suhina_plugins::trace!("vst3 drop: controller terminate");
            // SAFETY: a live controller, terminated once before it is released.
            let _ = unsafe { controller.terminate() };
        }
        suhina_plugins::trace!("vst3 drop: controller release");
        self.controller = None;
        suhina_plugins::trace!("vst3 drop: component terminate");
        // SAFETY: a live component, terminated once; it is released when `self` is dropped,
        // before the module (declared last) unloads the code.
        let _ = unsafe { self.component.terminate() };
        suhina_plugins::trace!("vst3 drop: terminated; releasing");
        let _ = &self.module;
    }
}

/// The audio thread: serves blocks from the link through the processor until `stop` is set
/// (and runs it after other instances when their blocks ask, [`crate::audio`]), then hands the
/// link back. When `flush` is set by then, the parameter values still waiting go to the
/// processor first, in a block of no samples (the SDK's way of passing parameters without
/// audio).
#[allow(clippy::too_many_arguments)]
fn serve(
    instance: u32,
    audio: AudioSide,
    mut served: Served,
    buses: (Vec<usize>, Vec<usize>),
    sample_rate: f64,
    (pending, applied, engine, mapping, midi_watch): (Pending, Notes, Notes, MidiMapping, Option<Arc<MidiWatch>>),
    (stop, flush): (&AtomicBool, &AtomicBool),
    watch: Option<Arc<Watch>>,
) -> Served {
    let processor = audio.0;
    let waiting = pending.clone();
    // SAFETY: a live, active processor, on the audio thread.
    let _ = unsafe { processor.setProcessing(1) };
    let mut block = Vst3Block::new(processor, &served, buses, sample_rate, (pending, applied, engine, mapping));
    block.watch = watch;
    block.midi_watch = midi_watch;
    if let Some(mut block) = crate::audio::serve(instance, block, &mut served, stop) {
        if flush.load(Ordering::Acquire) && waiting.lock().is_ok_and(|w| !w.is_empty()) {
            block.flush();
        }
        suhina_plugins::trace!("vst3 audio: setProcessing(0)");
        // SAFETY: as above.
        let _ = unsafe { block.processor.setProcessing(0) };
        suhina_plugins::trace!("vst3 audio: stopped");
    }
    served
}

/// The processing processor and what it needs to run a block, without allocating.
struct Vst3Block {
    processor: ComPtr<IAudioProcessor>,
    changes: BlockChanges,
    notes: BlockEvents,
    mapping: MidiMapping,
    max_frames: usize,
    link_channels: usize,
    /// One buffer per channel of every bus; the pointers in `inputs` and `outputs` point into
    /// these and stay valid, because the buffers are never resized (moving the vectors does not
    /// move what they hold).
    in_buffers: Vec<Vec<Vec<f32>>>,
    out_buffers: Vec<Vec<Vec<f32>>>,
    _in_pointers: Vec<Vec<*mut f32>>,
    _out_pointers: Vec<Vec<*mut f32>>,
    inputs: Vec<AudioBusBuffers>,
    outputs: Vec<AudioBusBuffers>,
    context: ProcessContext,
    /// With a trace, an ARA-bound plug-in's output summed up (`docs/debugging.md`).
    watch: Option<Arc<Watch>>,
    /// With a trace, the notes, controllers and parameter changes it was given, counted for
    /// the main thread to write once a second of audio (`docs/debugging.md`).
    midi_watch: Option<Arc<MidiWatch>>,
    /// The frames played since the last second of audio was marked for [`MidiWatch`].
    midi_frames: u64,
}

/// What a plug-in was given since the last trace line: events by kind, and the parameter
/// changes the controllers, pitch bend and pressure became through its MIDI mapping. Counted
/// on the audio thread without a lock; the main thread writes and clears it when a second of
/// audio has passed ([`MidiWatch::write`]).
struct MidiWatch {
    notes: AtomicU32,
    controllers: AtomicU32,
    bends: AtomicU32,
    pressures: AtomicU32,
    mapped: AtomicU32,
    /// The last mapped change's parameter, [`NO_PARAM`] for none, and its value's bits.
    last_id: AtomicU64,
    last_value: AtomicU64,
    /// A second of audio has passed since the last line.
    due: AtomicBool,
}

/// No mapped change since the last line: not any `ParamID`, which is 32 bits.
const NO_PARAM: u64 = u64::MAX;

impl Default for MidiWatch {
    fn default() -> Self {
        Self {
            notes: AtomicU32::new(0),
            controllers: AtomicU32::new(0),
            bends: AtomicU32::new(0),
            pressures: AtomicU32::new(0),
            mapped: AtomicU32::new(0),
            last_id: AtomicU64::new(NO_PARAM),
            last_value: AtomicU64::new(0),
            due: AtomicBool::new(false),
        }
    }
}

impl MidiWatch {
    /// On the main thread: when a second of audio has passed and anything came, writes the
    /// trace line, and starts the next second's count.
    fn write(&self, instance: u32) {
        if !self.due.swap(false, Ordering::AcqRel) {
            return;
        }
        let take = |count: &AtomicU32| count.swap(0, Ordering::AcqRel);
        let (notes, controllers, bends, pressures, mapped) =
            (take(&self.notes), take(&self.controllers), take(&self.bends), take(&self.pressures), take(&self.mapped));
        let last = match self.last_id.swap(NO_PARAM, Ordering::AcqRel) {
            NO_PARAM => None,
            id => Some((id as ParamID, f64::from_bits(self.last_value.load(Ordering::Acquire)))),
        };
        if notes + controllers + bends + pressures > 0 {
            suhina_plugins::trace!(
                "vst3 events {instance}: notes {notes}, controllers {controllers}, bends {bends}, pressure {pressures}; mapped {mapped}, last {last:?}"
            );
        }
    }
}

/// What an ARA-bound plug-in played since the last trace line, which the audio thread sets and
/// [`Vst3Plugin::trace_output`] reads: atomics, as the audio thread must not allocate, lock or
/// write a file.
#[derive(Default)]
struct Watch {
    frames: AtomicU64,
    /// The loudest sample, as `f32` bits.
    peak: AtomicU32,
    /// The song's place it was told, in samples, and whether it plays.
    song_samples: AtomicI64,
    playing: AtomicBool,
}

impl Watch {
    /// Raises the peak to the loudest sample of `output`. The block's peak starts from zero and
    /// is merged with `fetch_max`, so a [`Watch::take_peak`] between two blocks never has an old
    /// peak written back. The bits of a non-negative `f32` order like the numbers, and a NaN
    /// sample is passed over by `f32::max`.
    fn merge_peak(&self, output: &[f32]) {
        let peak = output.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
        self.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }

    /// The loudest sample since the last call, which starts the next interval from silence.
    fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    /// The frames played since the last call. A swap, so frames the audio thread adds meanwhile
    /// count for the next interval rather than being erased.
    fn take_frames(&self) -> u64 {
        self.frames.swap(0, Ordering::AcqRel)
    }
}

// SAFETY: the block is used by one audio thread at a time (behind its lock, `crate::audio`),
// which the VST3 threading model allows for `process`: hosts run processors on whichever of
// their audio threads is free. Its raw pointers point into buffers it owns, whose heap memory
// does not move with it, and the COM objects it holds are only touched from `process`.
unsafe impl Send for Vst3Block {}

impl Vst3Block {
    fn new(
        processor: ComPtr<IAudioProcessor>,
        served: &Served,
        buses: (Vec<usize>, Vec<usize>),
        sample_rate: f64,
        (pending, applied, engine, mapping): (Pending, Notes, Notes, MidiMapping),
    ) -> Self {
        let max_frames = served.max_frames() as usize;
        let (in_channels, out_channels) = buses;
        let mut in_buffers: Vec<Vec<Vec<f32>>> = in_channels.iter().map(|c| vec![vec![0.0; max_frames]; *c]).collect();
        let mut out_buffers: Vec<Vec<Vec<f32>>> =
            out_channels.iter().map(|c| vec![vec![0.0; max_frames]; *c]).collect();
        let mut in_pointers: Vec<Vec<*mut f32>> =
            in_buffers.iter_mut().map(|bus| bus.iter_mut().map(|c| c.as_mut_ptr()).collect()).collect();
        let mut out_pointers: Vec<Vec<*mut f32>> =
            out_buffers.iter_mut().map(|bus| bus.iter_mut().map(|c| c.as_mut_ptr()).collect()).collect();
        let bus_buffers = |pointers: &mut Vec<Vec<*mut f32>>| -> Vec<AudioBusBuffers> {
            pointers
                .iter_mut()
                .map(|channels| AudioBusBuffers {
                    numChannels: channels.len() as int32,
                    silenceFlags: 0,
                    __field0: AudioBusBuffers__type0 { channelBuffers32: channels.as_mut_ptr() },
                })
                .collect()
        };
        let inputs = bus_buffers(&mut in_pointers);
        let outputs = bus_buffers(&mut out_pointers);
        let mut context: ProcessContext = blank();
        context.sampleRate = sample_rate;
        context.tempo = 120.0;
        context.timeSigNumerator = 4;
        context.timeSigDenominator = 4;
        Self {
            processor,
            changes: BlockChanges::new(pending, applied, engine),
            notes: BlockEvents::new(),
            mapping,
            max_frames,
            link_channels: served.channels() as usize,
            in_buffers,
            out_buffers,
            _in_pointers: in_pointers,
            _out_pointers: out_pointers,
            inputs,
            outputs,
            context,
            watch: None,
            midi_watch: None,
            midi_frames: 0,
        }
    }
}

impl Vst3Block {
    /// Hands the processor the parameter values still waiting, in a block of no samples.
    fn flush(&mut self) {
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: 0,
            numInputs: self.inputs.len() as int32,
            numOutputs: self.outputs.len() as int32,
            inputs: self.inputs.as_mut_ptr(),
            outputs: self.outputs.as_mut_ptr(),
            inputParameterChanges: self.changes.take(std::iter::empty()),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: std::ptr::null_mut(),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut self.context,
        };
        // SAFETY: as in `process`, with no samples to read or write.
        let _ = unsafe { self.processor.process(&mut data) };
        self.changes.done();
    }
}

/// Puts the song's place the engine sent with the block into the plug-in's process context:
/// playing or stopped, the arrangement position in samples and in beats, the tempo, the bar, the
/// time signature and the loop. A block without it leaves the context as it was, counting on from the
/// last block.
fn follow_song(context: &mut ProcessContext, events: &[suhina_ipc::Event]) {
    use suhina_ipc::EventKind;
    use vst3::Steinberg::Vst::ProcessContext_::{StatesAndFlags, StatesAndFlags_ as Flags};
    #[allow(clippy::unnecessary_cast, reason = "the flags are signed on Windows")]
    let bits = |flag: StatesAndFlags| flag as u32;
    let mut told_beat = false;
    for event in events {
        match event.kind {
            EventKind::Transport { playing, position } => {
                let position = i64::try_from(position).unwrap_or(i64::MAX);
                context.projectTimeSamples = crate::ara::shown_position(position, context.sampleRate, playing);
                let playing_flag = bits(Flags::kPlaying);
                context.state = if playing { context.state | playing_flag } else { context.state & !playing_flag };
            }
            EventKind::Tempo { bpm } => {
                context.tempo = f64::from(bpm);
                context.state |= bits(Flags::kTempoValid);
            }
            EventKind::Beat { beat } => {
                context.projectTimeMusic = beat;
                context.state |= bits(Flags::kProjectTimeMusicValid);
                told_beat = true;
            }
            EventKind::Cycle { on, start, end } => {
                context.cycleStartMusic = f64::from(start);
                context.cycleEndMusic = f64::from(end);
                let cycle = bits(Flags::kCycleActive);
                context.state = if on { context.state | cycle } else { context.state & !cycle };
                context.state |= bits(Flags::kCycleValid);
            }
            EventKind::Bar { start, numerator, denominator, .. } => {
                context.barPositionMusic = start;
                context.timeSigNumerator = i32::from(numerator);
                context.timeSigDenominator = i32::from(denominator);
                context.state |= bits(Flags::kBarPositionValid) | bits(Flags::kTimeSigValid);
            }
            _ => {}
        }
    }
    // An older engine sends no beat: worked out from the samples at an even tempo.
    if !told_beat && context.sampleRate > 0.0 && context.tempo > 0.0 {
        context.projectTimeMusic = context.projectTimeSamples as f64 / context.sampleRate * context.tempo / 60.0;
        context.state |= bits(Flags::kProjectTimeMusicValid);
    }
}

impl Vst3Block {
    /// Sums up an ARA-bound plug-in's `output` of `frames` frames for the trace
    /// ([`Vst3Plugin::trace_output`]). Never allocates or locks.
    fn watch_output(&mut self, output: &[f32], frames: usize) {
        let Some(watch) = self.watch.as_ref() else { return };
        watch.merge_peak(output);
        use vst3::Steinberg::Vst::ProcessContext_::StatesAndFlags_ as Flags;
        #[allow(clippy::unnecessary_cast, reason = "the flags are signed on Windows")]
        let playing = self.context.state & (Flags::kPlaying as u32) != 0;
        watch.playing.store(playing, Ordering::Relaxed);
        watch.song_samples.store(self.context.projectTimeSamples, Ordering::Relaxed);
        watch.frames.fetch_add(frames as u64, Ordering::Release);
    }

    /// With a trace, counts what the block gives the plug-in, and marks each second of audio
    /// for the main thread to write ([`MidiWatch::write`]): whether a wheel moved on a
    /// controller reaches the plug-in, and as which parameter. Atomics only: real-time safe.
    fn watch_events(&mut self, events: &[suhina_ipc::Event], frames: usize) {
        use suhina_ipc::EventKind;
        let rate = self.context.sampleRate;
        let Some(watch) = self.midi_watch.as_deref() else { return };
        let count = |counter: &AtomicU32| counter.fetch_add(1, Ordering::Relaxed);
        for event in events {
            match event.kind {
                EventKind::NoteOn { .. } | EventKind::NoteOff { .. } => count(&watch.notes),
                EventKind::Controller { .. } => count(&watch.controllers),
                EventKind::PitchBend { .. } => count(&watch.bends),
                EventKind::Pressure { .. } => count(&watch.pressures),
                _ => continue,
            };
            if let Some((_, id, value)) = self.mapping.controller(event) {
                count(&watch.mapped);
                watch.last_value.store(value.to_bits(), Ordering::Release);
                watch.last_id.store(u64::from(id), Ordering::Release);
            }
        }
        self.midi_frames += frames as u64;
        if rate <= 0.0 || (self.midi_frames as f64) < rate {
            return;
        }
        self.midi_frames = 0;
        watch.due.store(true, Ordering::Release);
    }
}

impl crate::audio::Block for Vst3Block {
    fn process(&mut self, input: &[f32], events: &[suhina_ipc::Event], output: &mut [f32]) {
        let link_channels = self.link_channels;
        let frames = (input.len() / link_channels.max(1)).min(self.max_frames);
        for bus in self.in_buffers.iter_mut() {
            for channel in bus.iter_mut() {
                channel.fill(0.0);
            }
        }
        if let Some(main) = self.in_buffers.first_mut() {
            crate::audio::feed_main(input, link_channels, frames, main);
        }
        follow_song(&mut self.context, events);
        self.watch_events(events, frames);
        let mapping = &self.mapping;
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: frames as int32,
            numInputs: self.inputs.len() as int32,
            numOutputs: self.outputs.len() as int32,
            inputs: self.inputs.as_mut_ptr(),
            outputs: self.outputs.as_mut_ptr(),
            inputParameterChanges: self.changes.take(events.iter().filter_map(|e| mapping.change(e))),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: self.notes.fill(events),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut self.context,
        };
        // SAFETY: a live, processing processor on an audio thread of the process, one at a
        // time; every bus has a buffer per channel of at least `frames` samples, and all of it
        // outlives the call.
        let result = unsafe { self.processor.process(&mut data) };
        self.changes.done();
        self.context.projectTimeSamples += frames as i64;
        self.context.continousTimeSamples += frames as i64;
        if !ok(result) {
            return;
        }
        if let Some(main) = self.out_buffers.first() {
            crate::audio::collect_main(main, link_channels, frames, output);
        }
        self.watch_output(output, frames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trace_watch_merges_peaks_and_starts_each_interval_from_silence() {
        let watch = Watch::default();
        watch.merge_peak(&[0.25, -0.75, 0.5]);
        watch.merge_peak(&[0.5, f32::NAN]);
        watch.merge_peak(&[]);
        assert_eq!(watch.take_peak(), 0.75);
        assert_eq!(watch.take_peak(), 0.0);
        watch.merge_peak(&[-0.125]);
        assert_eq!(watch.take_peak(), 0.125);
    }

    /// The main thread resets the counters while the audio thread keeps adding to them: no
    /// frame is lost, and a peak that was taken never comes back.
    #[test]
    fn resetting_the_trace_watch_loses_no_frames_and_repeats_no_peak() {
        const BLOCKS: u32 = 200_000;
        let watch = Arc::new(Watch::default());
        let audio = {
            let watch = watch.clone();
            std::thread::spawn(move || {
                for block in 1..=BLOCKS {
                    watch.merge_peak(&[block as f32]);
                    watch.frames.fetch_add(1, Ordering::Release);
                }
            })
        };
        let (mut frames, mut peaks) = (0_u64, Vec::new());
        while !audio.is_finished() {
            frames += watch.take_frames();
            let peak = watch.take_peak();
            if peak > 0.0 {
                peaks.push(peak);
            }
        }
        audio.join().unwrap();
        frames += watch.take_frames();
        let peak = watch.take_peak();
        if peak > 0.0 {
            peaks.push(peak);
        }
        assert_eq!(frames, u64::from(BLOCKS), "frames were erased by a reset");
        assert_eq!(peaks.last().copied(), Some(BLOCKS as f32));
        assert!(peaks.windows(2).all(|pair| pair[0] < pair[1]), "an old peak was written back");
    }

    #[test]
    fn a_module_is_loaded_once_and_kept() {
        let (first, _) = open("test:vst3").unwrap();
        let (second, _) = open("test:vst3").unwrap();
        assert!(Rc::ptr_eq(&first, &second));
        let weak = Rc::downgrade(&first);
        drop((first, second));
        // Nothing uses it, and it stays loaded: the next open gets the same module, and its
        // `ExitDll` never runs while the process lives.
        let (third, _) = open("test:vst3").unwrap();
        assert!(weak.upgrade().is_some_and(|kept| Rc::ptr_eq(&kept, &third)));
    }

    #[test]
    fn class_ids_round_trip_as_hex() {
        let cid: TUID = [0x12, -1, 0, 0x7f, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, -128];
        let hex = cid_hex(&cid);
        let expected = if cfg!(windows) { "7F00FF12020104030506070809" } else { "12FF007F01020304050607080900" };
        assert!(hex.starts_with(&expected[..16]), "{hex}");
        assert_eq!(cid_from_hex(&hex), Some(cid));
        assert_eq!(cid_from_hex("12"), None);
        assert_eq!(cid_from_hex(&"zz".repeat(16)), None);
    }

    #[test]
    fn the_memory_stream_reads_writes_and_seeks() {
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>().unwrap_or_else(|| panic!("stream"));
        let mut bytes = *b"hello";
        let mut count = 0;
        // SAFETY: a live stream and buffers that outlive the calls.
        unsafe {
            assert_eq!(raw.write(bytes.as_mut_ptr().cast(), 5, &mut count), kResultOk);
        }
        assert_eq!((stream.bytes(), count), (b"hello".to_vec(), 5));
        let mut at = 0;
        // SAFETY: as above.
        unsafe {
            assert_eq!(raw.seek(1, IBStream_::IStreamSeekMode_::kIBSeekSet as int32, &mut at), kResultOk);
        }
        let mut out = [0u8; 8];
        // SAFETY: as above.
        unsafe {
            assert_eq!(raw.read(out.as_mut_ptr().cast(), 8, &mut count), kResultOk);
        }
        assert_eq!((&out[..4], count), (&b"ello"[..], 4));
        // SAFETY: as above.
        let before_start = unsafe { raw.seek(-10, IBStream_::IStreamSeekMode_::kIBSeekCur as int32, &mut at) };
        assert_eq!(before_start, kInvalidArgument);
        // SAFETY: as above.
        assert_eq!(unsafe { raw.tell(&mut at) }, kResultOk);
        assert_eq!(at, 5);
    }

    #[test]
    fn bundle_layout() {
        let missing = binary(Path::new("no-such.vst3"));
        assert!(missing.is_err());
    }

    #[test]
    fn a_macos_bundles_executable_is_found() {
        let dir = std::env::temp_dir().join(format!("suhina-vst3-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let stem = OsStr::new("Thing");
        let macos = dir.join("Thing.vst3").join("Contents").join("MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        // An empty folder holds no executable; the bundle is refused.
        assert_eq!(macos_binary(&macos, stem), None);
        // Named after the bundle without the extension, as most bundles have.
        std::fs::write(macos.join("Thing"), "not really a Mach-O").unwrap();
        assert_eq!(macos_binary(&macos, stem), Some(macos.join("Thing")));
        // On macOS the bundle then loads by that file, whatever its name says.
        if cfg!(target_os = "macos") {
            let found = binary(&dir.join("Thing.vst3")).ok();
            assert_eq!(found, Some(macos.join("Thing")));
        }
        // An executable under another name, which the bundle format allows; a hidden file
        // such as a stray .DS_Store is not taken for it.
        let renamed = dir.join("Other.vst3").join("Contents").join("MacOS");
        std::fs::create_dir_all(&renamed).unwrap();
        std::fs::write(renamed.join(".DS_Store"), "x").unwrap();
        std::fs::write(renamed.join("Other.vst3"), "not really a Mach-O").unwrap();
        assert_eq!(macos_binary(&renamed, stem), Some(renamed.join("Other.vst3")));
        // A folder that is not there holds nothing.
        assert_eq!(macos_binary(&dir.join("missing").join("MacOS"), stem), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
