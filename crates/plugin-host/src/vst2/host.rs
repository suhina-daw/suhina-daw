// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running a VST2 plug-in, on the same boundary as CLAP and VST3 (ADR 0006): the instance
//! lives in this process, its audio in [`crate::audio`]'s threads through the link, its
//! main-thread calls served from the process's main loop. Everything the interface lacks is
//! marked *assumed* as in the rest of this layer and listed in
//! [`docs/spec/plugin-vst2.md`](../../../docs/spec/plugin-vst2.md); the probe and the ignored
//! test against installed plug-ins are what can turn an assumption into an observation.
//!
//! The one shape the interface forces, which the other formats do not, is the audio-master
//! callback: one global function with only the effect pointer to say who is asking. The
//! audio thread says which instance it is serving in a thread-local set around each
//! `process` call (no lock, so the audio thread never takes one); a call from any other
//! thread — an editor's, a plug-in's own worker — finds the instance by its effect, under a
//! lock those threads may take. What the callback answers on the audio thread comes from
//! atomics and lock-free slots only.
//!
//! A plug-in is named `vst2:<file>` (a macOS bundle's executable is looked up as a VST3
//! bundle's is) or `test:vst2` for the built-in mock.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use suhina_ipc::{Event, EventKind, Served};
use suhina_plugins::ParamInfo;

use super::vestige::{self, AEffect, VstEvents, VstMidiEvent};
use super::{EffectOp, HOST_VERSION, HostOp, automation_state, c_str, can_do, effect_flags, process_level};

/// Most channels a plug-in may declare before its counts are cut off.
const MAX_CHANNELS: usize = 64;
/// Most parameters listed, and set from a program bank.
const MAX_PARAMS: usize = 4096;
/// Most bytes one state chunk may hold.
const MAX_CHUNK: usize = 256 * 1024 * 1024;
/// The room a plug-in's own edits have between main-loop drains.
const EDITS_ROOM: usize = 256;
/// The strings this host answers `audioMasterCanDo` with: it sends event lists and MIDI
/// events, answers the time, and takes the MIDI events a plug-in sends (counted for now: the
/// engine has no channel for a plug-in's MIDI out yet). *Assumed* spellings.
const CAN_DO: [&str; 4] =
    [can_do::HOST_SENDS_EVENTS, can_do::HOST_SENDS_MIDI, can_do::HOST_SENDS_TIME, can_do::HOST_RECEIVES_MIDI];

/// What a plug-in and its host share through the callback, reached from any thread: what the
/// audio thread writes in atomics and lock-free slots (never a lock), and what any other
/// thread may lock for.
pub(crate) struct Shared {
    /// A value the plug-in changed itself on the audio thread (its own automation, its
    /// editor's knob while a block runs): written only while a block of this instance runs
    /// (behind its lock in [`crate::audio`], so never by two threads at once), read by the
    /// main thread ([`crate::param_notes`], lock-free).
    notes: crate::param_notes::Notes,
    /// The same, from any other thread (an editor's gesture, a worker's): drained by
    /// [`Vst2Plugin::tick`].
    edits: Mutex<Vec<(u32, f32)>>,
    /// The plug-in said its inputs, outputs or latency changed: re-read on the main thread.
    io_changed: AtomicBool,
    /// The plug-in asked for idle calls (`effIdle`, from the main loop).
    need_idle: AtomicBool,
    /// The MIDI events a plug-in sent its host, which the engine has no channel for yet:
    /// counted, not kept.
    midi_out: AtomicU32,
    /// The effect's id and address, cached at load so no thread reads the plug-in's struct
    /// for them, and so a call from the audio thread can tell its own instance from another.
    unique_id: u32,
    effect: usize,
    /// The sample rate and block size as they stand, as bits and a count.
    sample_rate: AtomicU64,
    block_size: AtomicUsize,
    /// The song's place for the block being processed, field by field in atomics: written by
    /// the audio thread before each block, read by the callback on any thread.
    time: TimeSlot,
    /// Whether an editor of the plug-in is open: written by the main thread as it opens or
    /// closes one, read by the callback for a size request.
    has_editor: AtomicBool,
    /// Whether the main thread is restoring the plug-in's state right now: a plug-in's own
    /// report of the value just restored is not an edit (reading a state is neither a change
    /// nor an undo step, ADR 0010 decision 7), so the callback keeps it out of the records.
    restoring: AtomicBool,
    /// The window resize the plug-in asked for with `audioMasterSizeWindow`, pended for the
    /// main loop to apply: width << 16 | height, 0 for none.
    size_window: AtomicU64,
}

/// Packs a size into a pending `size_window` slot.
const fn pack_size(width: u32, height: u32) -> u64 {
    ((width as u64) << 16) | (height as u64)
}

/// Unpacks a pending `size_window` slot.
const fn unpack_size(packed: u64) -> (i32, i32) {
    ((packed >> 16) as i32, (packed & 0xffff) as i32)
}

/// The time information in atomics, one per field, so the audio thread can write it and the
/// callback read it without a lock. Fields a block did not tell stay as the last block left
/// them; a reader may see two blocks' fields mixed at a boundary, never a torn one.
struct TimeSlot {
    sample_pos: AtomicU64,
    sample_rate: AtomicU64,
    ppq_pos: AtomicU64,
    tempo: AtomicU64,
    bar_start: AtomicU64,
    cycle_start: AtomicU64,
    cycle_end: AtomicU64,
    /// The time signature's numerator and denominator packed together.
    time_sig: AtomicI32,
    flags: AtomicI32,
}

impl TimeSlot {
    fn new(sample_rate: f64) -> Self {
        let bits = |value: f64| AtomicU64::new(value.to_bits());
        Self {
            sample_pos: AtomicU64::new(0),
            sample_rate: bits(sample_rate),
            ppq_pos: AtomicU64::new(0),
            tempo: bits(120.0),
            bar_start: AtomicU64::new(0),
            cycle_start: AtomicU64::new(0),
            cycle_end: AtomicU64::new(0),
            time_sig: AtomicI32::new(4 << 16 | 4),
            flags: AtomicI32::new(0),
        }
    }

    /// The slot as the interface's struct, for the callback to answer with.
    fn load(&self) -> vestige::VstTimeInfo {
        let field = |slot: &AtomicU64| f64::from_bits(slot.load(Ordering::Relaxed));
        let signature = self.time_sig.load(Ordering::Relaxed);
        vestige::VstTimeInfo {
            samplePos: field(&self.sample_pos),
            sampleRate: field(&self.sample_rate),
            nanoSeconds: 0.0,
            ppqPos: field(&self.ppq_pos),
            tempo: field(&self.tempo),
            barStartPos: field(&self.bar_start),
            cycleStartPos: field(&self.cycle_start),
            cycleEndPos: field(&self.cycle_end),
            timeSigNumerator: signature >> 16,
            timeSigDenominator: signature & 0xffff,
            smpteOffset: 0,
            smpteFrameRate: 0,
            samplesToNextClock: 0,
            flags: self.flags.load(Ordering::Relaxed),
        }
    }
}

// The instance an audio thread is running the plug-in of, around its `process` call: the
// VST2 interface has one global callback and only the effect pointer to say who is asking,
// so the audio thread says which instance it serves here. Set and cleared per block, so a
// thread that runs another instance's block as a follower answers as that one. Const-built,
// so reading it never allocates or locks.
thread_local! {
    static SERVING: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

// Where `answer` writes the time information it answers with, per thread: the pointer it
// returns has to stay valid until the plug-in has read it, and every host answers from a
// buffer of its own. Valid until the same thread's next `GetTime` (*assumed* contract).
thread_local! {
    static TIME: RefCell<vestige::VstTimeInfo> = const {
        RefCell::new(vestige::VstTimeInfo {
            samplePos: 0.0, sampleRate: 0.0, nanoSeconds: 0.0, ppqPos: 0.0, tempo: 0.0,
            barStartPos: 0.0, cycleStartPos: 0.0, cycleEndPos: 0.0, timeSigNumerator: 0,
            timeSigDenominator: 0, smpteOffset: 0, smpteFrameRate: 0, samplesToNextClock: 0,
            flags: 0,
        })
    };
}

/// The instances this process hosts, by the effect each serves, for the calls that come from
/// any thread but the audio thread's own. Weak, so a plug-in left behind (its audio thread
/// stuck in it) keeps nothing alive.
static INSTANCES: Mutex<Vec<(usize, std::sync::Weak<Shared>)>> = Mutex::new(Vec::new());

/// The audio-master callback this host hands every VST2 plug-in. The answers' meanings are
/// the layer's assumptions, as the probe's are; which thread asks is told by [`SERVING`].
unsafe extern "C" fn host_callback(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    // The audio thread serves an instance now. The effect says who is asking; a call for the
    // instance this thread serves is answered from its lock-free slots, anything else by
    // looking the effect up. A call with a null effect is one from a plug-in's entry, which
    // cannot know its effect yet — real plug-ins make it and decline to instantiate when the
    // answers are dead (observed by the probe against installed plug-ins).
    let serving = SERVING.with(|serving| serving.borrow().clone());
    let shared = if effect.is_null() {
        None
    } else {
        match &serving {
            Some(serving) if serving.effect == effect as usize => Some(serving.clone()),
            _ => lookup(effect),
        }
    };
    match shared {
        Some(shared) => {
            let serving_this = serving.is_some_and(|s| Arc::ptr_eq(&s, &shared));
            answer(&shared, serving_this, opcode, index, value, ptr, opt)
        }
        None => answer_at_entry(opcode, ptr),
    }
}

/// What a plug-in is answered while its entry runs, before the effect exists: the host's
/// identity and what it can do, and nothing with an instance's part in it. The version is
/// the one an entry checks first — a dead answer there and the plug-in gives up (observed).
fn answer_at_entry(opcode: i32, ptr: *mut c_void) -> isize {
    match HostOp::from_value(opcode) {
        Some(HostOp::Version) => HOST_VERSION,
        // The id that is being loaded: the entry does not know it yet.
        Some(HostOp::CurrentId) => 0,
        Some(HostOp::CanDo) => {
            // SAFETY: the plug-in's C string, read with a bound.
            let asked = unsafe { c_str(ptr, 128) }.unwrap_or_default();
            isize::from(CAN_DO.contains(&asked.as_str()))
        }
        Some(HostOp::GetLanguage) => vestige::kVstLangEnglish as isize,
        Some(HostOp::GetVendorString) => {
            // SAFETY: the caller's buffer must hold the string and its NUL byte; writing
            // only that needs the least of the buffer, whose size is the op-code's
            // assumption.
            unsafe { super::write_text(ptr, "Suhina".len() + 1, "Suhina") };
            1
        }
        Some(HostOp::GetProductString) => {
            // SAFETY: as above.
            unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, "Suhina") };
            1
        }
        Some(HostOp::GetVendorVersion) => 1,
        // The tempo and the time information have no instance yet.
        Some(HostOp::TempoAt) | Some(HostOp::GetTime) => 0,
        // Anything else: noted, not answered.
        _ => 0,
    }
}

/// The instance an effect belongs to, when its address is registered.
fn lookup(effect: *mut AEffect) -> Option<Arc<Shared>> {
    if effect.is_null() {
        return None;
    }
    let key = effect as usize;
    let mut instances = INSTANCES.lock().ok()?;
    // A plug-in let go of without its entry being taken (its process is ending).
    instances.retain(|(_, shared)| shared.upgrade().is_some());
    instances.iter().find(|(served, _)| *served == key).and_then(|(_, shared)| shared.upgrade())
}

/// What this host answers for one call of the plug-in. `serving_this` says the call came from
/// the audio thread that is running this very instance, which is the only thread that may
/// write its lock-free slots.
fn answer(
    shared: &Shared,
    serving_this: bool,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    match HostOp::from_value(opcode) {
        // *Assumed* answer: 2400 stands for a 2.4 host.
        Some(HostOp::Version) => HOST_VERSION,
        // The effect's own id, cached at load.
        Some(HostOp::CurrentId) => isize::try_from(shared.unique_id).unwrap_or(0),
        // Noted; nothing to do.
        Some(HostOp::Idle)
        | Some(HostOp::WantMidi)
        | Some(HostOp::PinConnected)
        | Some(HostOp::VendorSpecific)
        | Some(HostOp::UpdateDisplay) => 0,
        // *Assumed:* the answer is a pointer to the host's time information: this thread's
        // copy of the slot, valid until its next `GetTime`.
        Some(HostOp::GetTime) => {
            let time = shared.time.load();
            TIME.with(|slot| {
                let mut slot = slot.borrow_mut();
                *slot = time;
                &*slot as *const vestige::VstTimeInfo as isize
            })
        }
        // A plug-in's MIDI out, which the engine has no channel for yet: counted, dropped.
        Some(HostOp::ProcessEvents) => {
            let _ = shared.midi_out.fetch_add(1, Ordering::Relaxed);
            0
        }
        // The tempo's unit in the answer is not observed; this host declares none.
        Some(HostOp::TempoAt) => 0,
        // The counts and the latency are re-read on the main thread.
        Some(HostOp::IOChanged) => {
            shared.io_changed.store(true, Ordering::Release);
            0
        }
        // Idle calls asked for and given, from the main loop.
        Some(HostOp::NeedIdle) => {
            shared.need_idle.store(true, Ordering::Release);
            0
        }
        // A knob the plug-in turned: the value it runs with now, reported to the session
        // (*assumed*: the parameter in `index`, the value in the call's `opt`). On the audio
        // thread it goes into the lock-free slots; anywhere else, under the lock those
        // threads may take.
        Some(HostOp::Automate) => {
            let id = u32::try_from(index).unwrap_or(0);
            if shared.restoring.load(Ordering::Acquire) {
                // The value the plug-in just had restored: not an edit.
                return 0;
            }
            if serving_this {
                shared.notes.note(id, f64::from(opt));
            } else if let Ok(mut edits) = shared.edits.lock() {
                // The values are re-read by the listing, so past the room the newest are
                // what is kept.
                if edits.len() >= EDITS_ROOM {
                    let cut = edits.len() / 2;
                    edits.drain(..cut);
                }
                edits.push((id, opt));
            }
            0
        }
        // The gesture around a knob: noted, nothing to do.
        Some(HostOp::BeginEdit) | Some(HostOp::EndEdit) => 0,
        // The editor's window grows to the plug-in's ask, applied by the main loop (*assumed*:
        // the width in the call's `index` and the height in its `value`; the pinned header
        // says only that the op-code exists). Only with an editor open.
        Some(HostOp::SizeWindow) => {
            let (width, height) = (usize::try_from(index).unwrap_or(0), usize::try_from(value).unwrap_or(0));
            if shared.has_editor.load(Ordering::Acquire)
                && (1..=16_384).contains(&width)
                && (1..=16_384).contains(&height)
            {
                shared.size_window.store(pack_size(width as u32, height as u32), Ordering::Release);
                1
            } else {
                0
            }
        }
        // *Assumed:* the rate answers as a whole number; the size as itself.
        Some(HostOp::GetSampleRate) => f64::from_bits(shared.sample_rate.load(Ordering::Relaxed)) as isize,
        Some(HostOp::GetBlockSize) => shared.block_size.load(Ordering::Relaxed) as isize,
        // This host adds no latency of its own.
        Some(HostOp::GetInputLatency) | Some(HostOp::GetOutputLatency) => 0,
        // *Assumed* values: a block's thread runs at the real-time level, anything else at
        // the user's.
        Some(HostOp::GetCurrentProcessLevel) => {
            if serving_this {
                process_level::REALTIME
            } else {
                process_level::USER
            }
        }
        // *Assumed* values; this host automates nothing itself.
        Some(HostOp::GetAutomationState) => automation_state::OFF,
        // *Assumed:* the answer 1 says the string was filled.
        Some(HostOp::GetVendorString) => {
            // SAFETY: the caller's buffer must hold the string and its NUL byte; writing
            // only that needs the least of the buffer, whose size is the op-code's
            // assumption.
            unsafe { super::write_text(ptr, "Suhina".len() + 1, "Suhina") };
            1
        }
        Some(HostOp::GetProductString) => {
            // SAFETY: as above.
            unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, "Suhina") };
            1
        }
        Some(HostOp::GetVendorVersion) => 1,
        // *Assumed:* `ptr` names what is asked; 1 is yes.
        Some(HostOp::CanDo) => {
            // SAFETY: the plug-in's C string, read with a bound.
            let asked = unsafe { c_str(ptr, 128) }.unwrap_or_default();
            isize::from(CAN_DO.contains(&asked.as_str()))
        }
        // From VeSTige: 1 is English.
        Some(HostOp::GetLanguage) => vestige::kVstLangEnglish as isize,
        // Anything else VeSTige names, and any op-code it does not: noted, not answered.
        _ => 0,
    }
}

/// An open editor's window and what it is at, kept on the main thread.
struct Vst2Editor {
    window: crate::window::Window,
    size: crate::window::Size,
    /// The new size the plug-in asked for and the main loop applied, to tell the engine once.
    resized: Option<crate::window::Size>,
}

/// What was loaded and must outlive the effect: a library, or the built-in mock. The
/// library and the mock's state are held — and never read — to keep the plug-in's code and
/// its state alive for as long as the effect is called.
#[allow(dead_code)]
enum Module {
    Library(libloading::Library),
    Mock(Box<AEffect>, Arc<super::test_vst2::Mock>),
}

/// A loaded, running VST2 plug-in and its audio thread.
pub(crate) struct Vst2Plugin {
    audio: Option<thread::JoinHandle<Served>>,
    stop: Arc<AtomicBool>,
    latency: u32,
    instance: u32,
    name: String,
    effect: *mut AEffect,
    /// What keeps the plug-in's code and its state alive, held for the effect's life and
    /// never read.
    #[allow(dead_code)]
    module: Module,
    /// The call fields and flags copied at load, so no thread reads the plug-in's struct for
    /// them again.
    dispatcher: Option<vestige::DispatcherFn>,
    process: Option<vestige::ProcessFn>,
    process_replacing: Option<vestige::ProcessFn>,
    set_parameter: Option<vestige::SetParameterFn>,
    get_parameter: Option<vestige::GetParameterFn>,
    flags: i32,
    /// (inputs, outputs) as the plug-in declared them at load, and its parameter count.
    buses: (usize, usize),
    params: usize,
    /// Each parameter's value as the plug-in was opened, before anything was set: the
    /// closest thing to a default the interface has (it names no op-code for one), and what
    /// a panel's reset restores.
    defaults: std::cell::RefCell<Vec<f32>>,
    version: i32,
    sample_rate: f64,
    max_frames: usize,
    shared: Arc<Shared>,
    /// The open editor's window, when one is open, and what it is at, on the main thread.
    editor: Option<Vst2Editor>,
    /// What [`Shared::notes`] has already reported (`Reads`).
    reads: crate::param_notes::Reads,
    /// Whether the plug-in ever asked for idle calls.
    idle: bool,
}

impl Vst2Plugin {
    /// Loads the plug-in `spec` names (`vst2:<file>` or the built-in mock), opens it for the
    /// link's block size at `sample_rate` and starts serving blocks from `served` on an audio
    /// thread, as instance `instance` of the process.
    pub(crate) fn load(spec: &str, instance: u32, served: Served, sample_rate: f64) -> Result<Self, String> {
        let max_frames = served.max_frames() as usize;
        let (module, effect) = open(spec)?;
        // SAFETY: the effect the entry gave back, read by value once, on this thread. The
        // call fields, flags and counts are copied here and never read from the plug-in's
        // struct again, so no thread races the plug-in over them.
        let read = unsafe { *effect };
        if read.magic != vestige::kEffectMagic {
            return Err(format!("the plug-in {spec:?} did not give a VST2 effect"));
        }
        // The counts it declares are hostile: over the cap and the arrays this host lays out
        // cannot hold them, so the plug-in is refused rather than clamped to a shape its own
        // code would not stay inside.
        let inputs = usize::try_from(read.numInputs).unwrap_or(0);
        let outputs = usize::try_from(read.numOutputs).unwrap_or(0);
        if let Some(what) = channel_cap(inputs, outputs) {
            return Err(format!("the plug-in {spec:?} {what}"));
        }
        let count = |value: i32| usize::try_from(value).unwrap_or(0).min(MAX_CHANNELS);
        let shared = Arc::new(Shared {
            notes: Arc::new(crate::param_notes::ParamNotes::new()),
            edits: Mutex::new(Vec::new()),
            io_changed: AtomicBool::new(false),
            need_idle: AtomicBool::new(false),
            midi_out: AtomicU32::new(0),
            unique_id: read.uniqueID as u32,
            has_editor: AtomicBool::new(false),
            restoring: AtomicBool::new(false),
            size_window: AtomicU64::new(0),
            effect: effect as usize,
            sample_rate: AtomicU64::new(sample_rate.to_bits()),
            block_size: AtomicUsize::new(max_frames),
            time: TimeSlot::new(sample_rate),
        });
        // Register before the plug-in is opened: a plug-in may call its host as it opens.
        if let Ok(mut instances) = INSTANCES.lock() {
            instances.retain(|(served, _)| *served != effect as usize);
            instances.push((effect as usize, Arc::downgrade(&shared)));
        }
        let mut plugin = Self {
            audio: None,
            stop: Arc::new(AtomicBool::new(false)),
            latency: latency_of(&read),
            instance,
            name: String::new(),
            effect,
            module,
            editor: None,
            dispatcher: read.dispatcher,
            process: read.process,
            process_replacing: read.processReplacing,
            set_parameter: read.setParameter,
            get_parameter: read.getParameter,
            flags: read.flags,
            buses: (count(read.numInputs), count(read.numOutputs)),
            params: usize::try_from(read.numParams).unwrap_or(0).min(MAX_PARAMS),
            defaults: std::cell::RefCell::new(Vec::new()),
            version: read.version,
            sample_rate,
            max_frames,
            shared,
            reads: crate::param_notes::ParamNotes::fresh_reads(),
            idle: false,
        };
        // Open it and set its sizes, as a host does before anything else (*assumed*: the rate
        // in the call's float, the size in its value).
        plugin.send(EffectOp::Open, 0, 0, std::ptr::null_mut(), 0.0)?;
        // The defaults: what each parameter was as it opened, before anything is set.
        plugin.defaults = std::cell::RefCell::new(
            (0..plugin.params).map(|i| plugin.parameter(i32::try_from(i).unwrap_or(0)).unwrap_or_default()).collect(),
        );
        plugin.send(EffectOp::SetSampleRate, 0, 0, std::ptr::null_mut(), sample_rate as f32)?;
        let size = isize::try_from(max_frames).unwrap_or(0);
        plugin.send(EffectOp::SetBlockSize, 0, size, std::ptr::null_mut(), 0.0)?;
        // Its name, for errors and traces; a plug-in that gives none keeps its file's.
        let mut field = vec![0 as std::ffi::c_char; vestige::VestigeMaxNameLen];
        // SAFETY: the buffer is this function's own, of the size the op-code is assumed to
        // give.
        let _ = plugin.send(EffectOp::GetEffectName, 0, 0, field.as_mut_ptr().cast::<c_void>(), 0.0);
        plugin.name = super::field_text(&field);
        if plugin.name.is_empty() {
            plugin.name = spec.strip_prefix("vst2:").unwrap_or(spec).to_string();
        }
        plugin.start_audio(served)?;
        Ok(plugin)
    }

    /// Starts an audio thread that serves blocks from `served`, resuming the plug-in for them
    /// and suspending it when the thread stops (a reconfigure, an unload).
    fn start_audio(&mut self, served: Served) -> Result<(), String> {
        self.stop = Arc::new(AtomicBool::new(false));
        let stop = self.stop.clone();
        let block = Vst2Block {
            effect: self.effect,
            dispatcher: self.dispatcher,
            process: self.process,
            process_replacing: self.process_replacing,
            set_parameter: self.set_parameter,
            flags: self.flags,
            shared: self.shared.clone(),
            max_frames: self.max_frames,
            link_channels: 0,
            buses: self.buses,
            in_buffers: Vec::new(),
            out_buffers: Vec::new(),
            in_pointers: Vec::new(),
            out_pointers: Vec::new(),
            midis: Vec::new(),
            events: EventBuffer::new(suhina_ipc::MAX_EVENTS.min(1024)),
            time: Told::default(),
        }
        .ready(&served);
        let instance = self.instance;
        let audio = thread::Builder::new()
            .name("plug-in audio".into())
            .spawn(move || {
                let _realtime = crate::real_time_thread();
                serve(instance, block, served, &stop)
            })
            .map_err(|e| e.to_string())?;
        self.audio = Some(audio);
        Ok(())
    }

    /// One dispatcher call to the plug-in, from the main thread. `Err` when it has no
    /// dispatcher; a call that crashes takes this process, as any format's would.
    fn send(&mut self, op: EffectOp, index: i32, value: isize, ptr: *mut c_void, opt: f32) -> Result<isize, String> {
        let Some(dispatcher) = self.dispatcher else { return Err(format!("{}: no dispatcher", op.name())) };
        // SAFETY: the dispatcher of the plug-in this host holds, with this call's arguments.
        Ok(unsafe { dispatcher(self.effect, op.value(), index, value, ptr, opt) })
    }

    /// One `getParameter` call, from the main thread.
    fn parameter(&self, index: i32) -> Result<f32, String> {
        let Some(get) = self.get_parameter else { return Err("the plug-in has no getParameter".into()) };
        // SAFETY: the plug-in's own call, with the effect it belongs to.
        Ok(unsafe { get(self.effect, index) })
    }

    /// One `setParameter` call, from the main thread: the one path a value reaches a running
    /// plug-in by, which the interface expects to be callable while it processes (*assumed*).
    fn set_value(&self, index: i32, value: f32) -> Result<(), String> {
        let Some(set) = self.set_parameter else { return Err("the plug-in has no setParameter".into()) };
        // SAFETY: the plug-in's own call, with the effect it belongs to.
        unsafe { set(self.effect, index, value) };
        Ok(())
    }

    /// The plug-in's parameters as they stand. The range is *assumed* to be 0 to 1 (the
    /// pinned header says nothing about a range), and the default is the value the plug-in
    /// was opened with, captured once at load (the header names no op-code to ask for one).
    pub(crate) fn params(&mut self) -> Result<Vec<ParamInfo>, String> {
        let mut out = Vec::new();
        for index in 0..self.params {
            let index = i32::try_from(index).unwrap_or(0);
            let mut name = vec![0 as std::ffi::c_char; vestige::VestigeMaxLabelLen];
            let mut text = vec![0 as std::ffi::c_char; vestige::VestigeMaxLabelLen];
            // The buffers are this function's own, of the sizes the op-codes are assumed to
            // give.
            let _ = self.send(EffectOp::GetParamName, index, 0, name.as_mut_ptr().cast::<c_void>(), 0.0);
            let _ = self.send(EffectOp::GetParamDisplay, index, 0, text.as_mut_ptr().cast::<c_void>(), 0.0);
            let value = f64::from(self.parameter(index)?);
            let automatable = self
                .send(EffectOp::CanBeAutomated, index, 0, std::ptr::null_mut(), 0.0)
                .map(|answer| answer != 0)
                .unwrap_or(true);
            let default = self.defaults.borrow().get(index as usize).copied().map(f64::from).unwrap_or(value);
            out.push(ParamInfo {
                id: u32::try_from(index).unwrap_or(0),
                name: super::field_text(&name),
                group: String::new(),
                min: 0.0,
                max: 1.0,
                default,
                value,
                text: super::field_text(&text),
                steps: 0,
                automatable,
                read_only: false,
                hidden: false,
            });
        }
        Ok(out)
    }

    /// Sets one parameter, in the plug-in's own range.
    pub(crate) fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        let index = i32::try_from(id).map_err(|_| format!("no parameter {id}"))?;
        if usize::try_from(index).unwrap_or(usize::MAX) >= self.params {
            return Err(format!("no parameter {id}"));
        }
        self.set_value(index, value.clamp(0.0, 1.0) as f32)?;
        let value = f64::from(self.parameter(index)?);
        let default = self.defaults.borrow().get(id as usize).copied().map(f64::from).unwrap_or(value);
        Ok(ParamInfo {
            id,
            name: String::new(),
            group: String::new(),
            min: 0.0,
            max: 1.0,
            default,
            value,
            text: String::new(),
            steps: 0,
            automatable: true,
            read_only: false,
            hidden: false,
        })
    }

    /// What the plug-in changed itself since the last call, as (parameter, value) pairs, and
    /// the idle calls it asked for served: called from the main loop every turn.
    pub(crate) fn tick(&mut self) -> Vec<(u32, f64)> {
        // A plug-in that asked for idle calls gets them, between messages, where its editor's
        // work belongs (VeSTige's `effIdle`).
        if self.idle || self.shared.need_idle.load(Ordering::Acquire) {
            self.idle = true;
            let _ = self.send(EffectOp::Idle, 0, 0, std::ptr::null_mut(), 0.0);
        }
        let mut changes: Vec<(u32, f64)> = Vec::new();
        if let Ok(mut edits) = self.shared.edits.lock() {
            changes.extend(edits.drain(..).map(|(id, value)| (id, f64::from(value))));
        }
        changes.extend(self.shared.notes.take(&mut self.reads));
        changes
    }

    /// Delay the plug-in adds, in frames, as it last reported.
    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    /// A latency the plug-in reported since this was last asked (`audioMasterIOChanged`).
    /// Its channel counts are re-read with it; if they changed, the buffers stay as they
    /// were — a limit of this first version, noted in the spec.
    pub(crate) fn latency_changed(&mut self) -> Option<u32> {
        if !self.shared.io_changed.swap(false, Ordering::AcqRel) {
            return None;
        }
        // SAFETY: the effect this host holds, read by value once, on this thread.
        let read = unsafe { *self.effect };
        let (inputs, outputs) = (
            usize::try_from(read.numInputs).unwrap_or(0).min(MAX_CHANNELS),
            usize::try_from(read.numOutputs).unwrap_or(0).min(MAX_CHANNELS),
        );
        if (inputs, outputs) != self.buses {
            suhina_plugins::trace!(
                "vst2 {}: its {} in and {} out became {inputs} and {outputs}; the buffers stay as they were",
                self.instance,
                self.buses.0,
                self.buses.1
            );
        }
        let now = latency_of(&read);
        if now != self.latency {
            self.latency = now;
            Some(now)
        } else {
            None
        }
    }

    /// Stops the audio thread (which suspends the plug-in), sets the new rate and size, and
    /// starts a new audio thread on the same link; the latency as it stands then.
    pub(crate) fn reconfigure(&mut self, sample_rate: f64) -> Result<u32, String> {
        if self.sample_rate == sample_rate {
            return Ok(self.latency);
        }
        if !self.stop_audio(crate::UNLOAD_WAIT) {
            return Err("its audio thread does not stop".into());
        }
        let served =
            self.audio.take().ok_or("it has no audio thread")?.join().map_err(|_| "its audio thread failed")?;
        self.send(EffectOp::SetSampleRate, 0, 0, std::ptr::null_mut(), sample_rate as f32)?;
        let size = isize::try_from(self.max_frames).unwrap_or(0);
        self.send(EffectOp::SetBlockSize, 0, size, std::ptr::null_mut(), 0.0)?;
        self.sample_rate = sample_rate;
        self.shared.sample_rate.store(sample_rate.to_bits(), Ordering::Relaxed);
        self.shared.time.sample_rate.store(sample_rate.to_bits(), Ordering::Relaxed);
        // SAFETY: as at load, on this thread, its audio stopped.
        let read = unsafe { *self.effect };
        self.latency = latency_of(&read);
        self.start_audio(served)?;
        Ok(self.latency)
    }

    /// Asks the audio thread to stop and waits up to `wait` for it; whether it stopped.
    pub(crate) fn stop_audio(&mut self, wait: Duration) -> bool {
        crate::audio_stopped(&self.stop, self.audio.as_ref(), wait)
    }

    /// The plug-in's state as the standard VST2 bank, the form the model keeps and a VST3
    /// replacement reads: its own chunk when it says it has one (*assumed* flag and
    /// op-codes), or its parameters as one program.
    pub(crate) fn state(&mut self) -> Result<Vec<u8>, String> {
        let (id, version, params) = (self.shared.unique_id, self.version, self.params);
        let preset = |kind, current| suhina_plugins::vst2::Preset {
            kind,
            id,
            version,
            programs: 1,
            current,
            parameters: params,
            little_endian: false,
        };
        if self.flags & effect_flags::HAS_CHUNKS != 0 {
            let mut pointer: *const c_void = std::ptr::null();
            let size = self.send(EffectOp::GetChunk, 0, 0, &mut pointer as *mut *const c_void as *mut c_void, 0.0)?;
            let size = usize::try_from(size).unwrap_or(0);
            // A size without the pointer it says it has is not a state to save: the caller
            // is told, and nothing fabricated is written in its place.
            if size > MAX_CHUNK || (size > 0 && pointer.is_null()) {
                return Err(format!("the plug-in said {size} bytes of state but gave no readable chunk"));
            }
            let mut chunk = vec![0u8; size];
            for (slot, i) in chunk.iter_mut().zip(0..size) {
                let byte = pointer.cast::<u8>().wrapping_add(i);
                // SAFETY: one of the `size` bytes the plug-in said its chunk has, at the
                // pointer it gave.
                *slot = unsafe { *byte };
            }
            let current = self.program();
            return suhina_plugins::vst2::bank(&preset(suhina_plugins::vst2::CHUNK_BANK, current), &chunk);
        }
        // Its parameters as one program (the other programs are not saved — a limit of this
        // first version, in the spec).
        let current = self.program();
        let mut buffer = Vec::with_capacity(28 + 4 * params);
        let mut name = vec![0 as std::ffi::c_char; 28];
        // SAFETY: this function's buffer, of the program name's size.
        let _ = self.send(EffectOp::GetProgramName, 0, 0, name.as_mut_ptr().cast::<c_void>(), 0.0);
        for byte in name.iter().take(28) {
            buffer.push(*byte as u8);
        }
        for index in 0..params {
            let value = self.parameter(i32::try_from(index).unwrap_or(0))?;
            buffer.extend_from_slice(&value.to_bits().to_be_bytes());
        }
        suhina_plugins::vst2::bank(&preset(suhina_plugins::vst2::PROGRAM_BANK, current), &buffer)
    }

    /// The program the plug-in is on, as the bank's `current` field wants it.
    fn program(&mut self) -> i32 {
        self.send(EffectOp::GetProgram, 0, 0, std::ptr::null_mut(), 0.0)
            .ok()
            .and_then(|answer| i32::try_from(answer).ok())
            .unwrap_or(0)
    }

    /// Hands the plug-in a state: a chunk bank's chunk, a program bank's current program
    /// (set as the program, then its values as parameters — *assumed* order), or a bare chunk.
    pub(crate) fn set_state(&mut self, data: &[u8]) -> Result<(), String> {
        // A plug-in's own report of the value being restored is not an edit: the guard is
        // set for the whole restore, on this main thread, and the callback keeps those out
        // of the records (ADR 0010 decision 7).
        self.shared.restoring.store(true, Ordering::Release);
        let restored = self.set_state_inner(data);
        self.shared.restoring.store(false, Ordering::Release);
        restored
    }

    fn set_state_inner(&mut self, data: &[u8]) -> Result<(), String> {
        if let Some(chunk) = suhina_plugins::vst2::chunk(data) {
            let size = isize::try_from(chunk.len()).unwrap_or(0);
            let taken = self.send(EffectOp::SetChunk, 0, size, chunk.as_ptr().cast::<c_void>().cast_mut(), 0.0)?;
            // *Assumed:* a non-zero answer is a refusal. A plug-in that refuses runs with its
            // defaults and the caller is told, not silently restored.
            if taken != 0 {
                return Err("the plug-in refused the state".into());
            }
            return Ok(());
        }
        if let Some((current, values)) = suhina_plugins::vst2::program(data) {
            let program = i32::try_from(current).unwrap_or(0);
            let _ = self.send(EffectOp::BeginSetProgram, 0, 0, std::ptr::null_mut(), 0.0);
            let program = isize::try_from(program).unwrap_or(0);
            let _ = self.send(EffectOp::SetProgram, 0, program, std::ptr::null_mut(), 0.0);
            let restored: Result<(), String> = values
                .into_iter()
                .enumerate()
                .take(self.params)
                .try_for_each(|(index, value)| self.set_value(i32::try_from(index).unwrap_or(0), value));
            // The bracket closes however the values went: without the end, the plug-in is
            // left thinking the program change is still going on.
            let _ = self.send(EffectOp::EndSetProgram, 0, 0, std::ptr::null_mut(), 0.0);
            restored?;
            return Ok(());
        }
        // Not a bank: a bare chunk, as a plug-in's own file would hold. A standard bank's
        // shape first — `CcnK` and a bank kind in their places — is refused when neither
        // reader took it: it stands for a bank that is cut short or lies, and the plug-in
        // would run with whatever raw bytes remain, saved as the state.
        if bank_shaped(data) {
            return Err("the state's bank is not complete".into());
        }
        if !data.is_empty() {
            let size = isize::try_from(data.len()).unwrap_or(0);
            let taken = self.send(EffectOp::SetChunk, 0, size, data.as_ptr().cast::<c_void>().cast_mut(), 0.0)?;
            if taken != 0 {
                return Err("the plug-in refused the state".into());
            }
        }
        Ok(())
    }

    /// Opens the plug-in's editor as `mode` says, as the CLAP host does: the window first,
    /// then the op-code `effEditOpen` given the window's native handle — a Win32 `HWND` on
    /// Windows, the content view's pointer on macOS, the X11 id on Linux (*all assumed*: the
    /// per-platform handle is not in the pinned header; the editor tests against the mock and
    /// the ignored test against installed editors are what settle the conventions). The size
    /// comes from `effEditGetRect`'s rect first (*assumed*: four 16-bit values).
    pub(crate) fn open_editor(&mut self, mode: crate::window::Mode) -> Result<crate::window::Size, String> {
        if let Some(size) = self.editor.as_ref().map(|editor| editor.size) {
            return Ok(size);
        }
        let before = self.edit_rect()?;
        let size = before.size();
        let window = crate::window::Window::open_as(mode, &self.name, size, false)
            .map_err(|e| format!("the editor did not open: {e}"))?;
        self.shared.has_editor.store(true, Ordering::Release);
        // SAFETY: the window's native handle, which stays open while the editor does.
        let opened =
            self.send(EffectOp::EditOpen, 0, 0, window.handle(), 0.0).map(|answer| answer != 0).unwrap_or(true);
        if !opened {
            self.editor_close();
            return Err("the plug-in's editor did not open".into());
        }
        // Many plug-ins fill their rect only as they open: asked again, the editor's real
        // size is what the window takes now.
        let mut size = size;
        if let Ok(after) = self.edit_rect() {
            let real = after.size();
            if real != size {
                crate::window::resize(window.id(), real);
                size = real;
            }
        }
        self.editor = Some(Vst2Editor { window, size, resized: None });
        Ok(size)
    }

    /// The editor's rectangle, as `effEditGetRect` filled it. Most plug-ins are around 300 ×
    /// 200 depending on third-party UI conventions; a refused rect keeps the window default.
    fn edit_rect(&mut self) -> Result<EditorRect, String> {
        // The op-code's `ptr` is a pointer slot the plug-in fills with a pointer to its own
        // rect (*assumed*, observed through the editor tests: the first read took the
        // pointer's bytes for the rect and made a window one pixel wide).
        let mut slot: *const EditorRect = std::ptr::null();
        // SAFETY: the buffer is this function's own, the pointer slot the op-code fills.
        let answer = self.send(EffectOp::EditGetRect, 0, 0, &mut slot as *mut *const EditorRect as *mut c_void, 0.0);
        if slot.is_null() {
            return Ok(EditorRect::BLANK);
        }
        // SAFETY: the rect the plug-in pointed the host at, read by value, its 8 bytes.
        let rect = unsafe { *slot };
        suhina_plugins::trace!(
            "vst2 rect: top {}, left {}, bottom {}, right {}",
            rect.top,
            rect.left,
            rect.bottom,
            rect.right
        );
        match answer {
            Ok(answer) if answer != 0 => Ok(rect),
            Err(why) => Err(format!("the editor's rect is refused: {why}")),
            _ => Ok(rect),
        }
    }

    /// Puts an embedded editor where the engine says.
    pub(crate) fn place_editor(&self, placement: crate::window::Placement) {
        if let Some(editor) = &self.editor {
            crate::window::place(editor.window.id(), placement);
        }
    }

    /// The room the engine has for an embedded editor: a VST2 has no op-code to ask a plug-in
    /// to resize its editor that the pinned header names, so this takes none (the spec's open
    /// questions).
    pub(crate) fn fit_editor(&mut self, _size: crate::window::Size) {}

    /// A size the editor got since this was last asked: the plug-in's `audioMasterSizeWindow`
    /// ask, applied by the main loop.
    pub(crate) fn editor_resized(&mut self) -> Option<crate::window::Size> {
        self.editor.as_mut().and_then(|editor| editor.resized.take())
    }

    /// Keeps an open editor in step with its window and pumps the idle calls a VST2 editor's
    /// drawing belongs to; whether the user closed its window. Called from the main loop
    /// every turn while the editor is open.
    pub(crate) fn editor_tick(&mut self) -> bool {
        let Some(window) = self.editor.as_ref().map(|editor| (editor.window.id(), editor.size)) else { return false };
        let (window_id, _) = window;
        // The plug-in's size ask, applied now that the main loop is here for it.
        let packed = self.shared.size_window.swap(0, Ordering::AcqRel);
        if packed != 0 {
            let (width, height) = unpack_size(packed);
            let got =
                crate::window::Size { width: width.clamp(1, 16_384) as u32, height: height.clamp(1, 16_384) as u32 };
            if let Some(editor) = self.editor.as_mut().filter(|editor| got != editor.size) {
                crate::window::resize(window_id, got);
                editor.size = got;
                editor.resized = Some(got);
            }
        }
        // The idle calls the editor's drawing work falls out from (*assumed*: served from the
        // host's idle loop, as the interface's own `effEditIdle` says).
        let _ = self.send(EffectOp::EditIdle, 0, 0, std::ptr::null_mut(), 0.0);
        // The user's close of the window, told the engine like the other formats do: the
        // editor is taken down here, as a close through the panel is — the op-code goes out
        // and the window leaves the screen (Windows' WM_CLOSE only marked the request;
        // macOS's user close only hid the window, which is still this struct's). Taken down
        // and told to the engine, a closed editor is not open for anyone, and "Open Editor"
        // asks the plug-in afresh.
        let closed = crate::window::take_close_request(window_id);
        if closed {
            self.close_editor();
        }
        closed
    }

    fn editor_close(&mut self) {
        self.shared.has_editor.store(false, Ordering::Release);
        self.shared.size_window.store(0, Ordering::Release);
        let _ = self.send(EffectOp::EditClose, 0, 0, std::ptr::null_mut(), 0.0);
    }

    /// Closes the editor, if one is open: the op-code first, then the window. The
    /// plug-in's `effEditClose` goes out while the window it was opened with still exists,
    /// whose handle it holds to the end (tearing its own things down with it); the
    /// window's destruction after is what takes a user-closed one off the screen.
    pub(crate) fn close_editor(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        self.editor_close();
        drop(editor.window);
    }

    /// Closes the plug-in and lets go of it. Only called when its audio thread stopped; one
    /// that did not is left as it is (never closed or freed under a running plug-in).
    pub(crate) fn close(&mut self) {
        let _ = self.send(EffectOp::Close, 0, 0, std::ptr::null_mut(), 0.0);
        if let Ok(mut instances) = INSTANCES.lock() {
            instances.retain(|(served, _)| *served != self.effect as usize);
        }
    }
}

/// The refusal a claimed channel count carries: `Some` what is wrong when either count runs
/// over the cap the arrays are laid out to hold. The counts are hostile numbers, and, like
/// the block they would serve, one over the cap is refused, not clamped to a shape the
/// plug-in's own code would not stay inside.
fn channel_cap(inputs: usize, outputs: usize) -> Option<String> {
    if inputs <= MAX_CHANNELS && outputs <= MAX_CHANNELS {
        return None;
    }
    Some(format!("declares {inputs} inputs and {outputs} outputs, over the {MAX_CHANNELS}-channel cap"))
}

/// Whether `data`'s shape is a standard bank: `CcnK` in its place, then a bank kind
/// (`FBCh`, a chunk bank, or `FxBk`, a bank of programs) in its own. The shapes the bank
/// readers take before they read further, or the shape a cut-short or lying bank carries.
fn bank_shaped(data: &[u8]) -> bool {
    data.get(..4) == Some(b"CcnK".as_slice())
        && (data.get(8..12) == Some(b"FBCh".as_slice()) || data.get(8..12) == Some(b"FxBk".as_slice()))
}

/// The latency field of an effect as this layer assumes it: the first of VeSTige's twelve
/// "zero" bytes, native byte order, negative read as none. The pinned header names no latency
/// field; the probe reports the twelve for every plug-in it meets, which is what settles
/// where a real one keeps its delay.
fn latency_of(effect: &AEffect) -> u32 {
    let bytes = [effect.empty3[0] as u8, effect.empty3[1] as u8, effect.empty3[2] as u8, effect.empty3[3] as u8];
    i32::from_ne_bytes(bytes).max(0) as u32
}

/// Serves `served` on the calling thread, `instance`'s audio thread: resuming the plug-in for
/// its blocks and suspending it when the thread stops. Returns the link for the next thread.
fn serve(instance: u32, block: Vst2Block, mut served: Served, stop: &AtomicBool) -> Served {
    let shared = block.shared.clone();
    // Resume, and open the process cycle, as a host does before the first block (*assumed*
    // order; the pinned header says only that the op-codes exist). A host ask the plug-in
    // makes in these dispatches is served from its lock-free slots, as a block's asks are.
    dispatch(&shared, block.effect, block.dispatcher, EffectOp::MainsChanged, 1);
    dispatch(&shared, block.effect, block.dispatcher, EffectOp::StartProcess, 0);
    if let Some(block) = crate::audio::serve(instance, block, &mut served, stop) {
        // SAFETY: as above, after the last block.
        dispatch(&shared, block.effect, block.dispatcher, EffectOp::StopProcess, 0);
        // SAFETY: as above.
        dispatch(&shared, block.effect, block.dispatcher, EffectOp::MainsChanged, 0);
    }
    served
}

/// One dispatcher call on the audio thread (`MainsChanged`, `StartProcess`, `StopProcess`):
/// the serving state holds while the plug-in runs, so the host asks it makes there are
/// answered from the lock-free slots and never the instance table's lock.
fn dispatch(
    shared: &Arc<Shared>,
    effect: *mut AEffect,
    dispatcher: Option<vestige::DispatcherFn>,
    op: EffectOp,
    value: isize,
) {
    SERVING.with(|serving| {
        *serving.borrow_mut() = Some(shared.clone());
        // SAFETY: the plug-in's dispatcher, with this call's arguments, on this audio
        // thread, with the serving state before it.
        unsafe { call(dispatcher, effect, op, value) };
        // The dispatch's asks are served; the rest of this thread's time is no instance's.
        *serving.borrow_mut() = None;
    });
}

/// One dispatcher call from the audio thread, for the cycle around the blocks.
unsafe fn call(dispatcher: Option<vestige::DispatcherFn>, effect: *mut AEffect, op: EffectOp, value: isize) {
    let Some(dispatcher) = dispatcher else { return };
    // SAFETY: the plug-in's dispatcher with this call's arguments, on the thread that serves
    // its blocks.
    unsafe { dispatcher(effect, op.value(), 0, value, std::ptr::null_mut(), 0.0) };
}

/// The editor's rectangle as `effEditGetRect` fills it (*assumed* four 16-bit values, top,
/// left, bottom and right — the pinned header has the op-code but not the shape, and this is
/// what the probe and the editor test settle).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct EditorRect {
    top: i16,
    left: i16,
    bottom: i16,
    right: i16,
}

impl EditorRect {
    const BLANK: Self = Self { top: 0, left: 0, bottom: 0, right: 0 };

    /// The inside the rectangle says, cut to the room a window has.
    fn size(&self) -> crate::window::Size {
        crate::window::Size {
            width: i32::from(self.right).saturating_sub(i32::from(self.left)).clamp(1, 16_384) as u32,
            height: i32::from(self.bottom).saturating_sub(i32::from(self.top)).clamp(1, 16_384) as u32,
        }
    }
}

/// The event list a block's MIDI events go in, reused every block: the `VstEvents` head and
/// the event pointers in line behind it in one allocation (where the interface's flexible
/// array member puts them), the events themselves in another that never moves. The pointers
/// are written once, when the buffer is made; only the count and the events change per block.
struct EventBuffer {
    list: Box<[u8]>,
    events: Box<[VstMidiEvent]>,
}

impl EventBuffer {
    fn new(room: usize) -> Self {
        let head = std::mem::size_of::<VstEvents>();
        let pointer = std::mem::size_of::<*mut vestige::VstEvent>();
        let mut list = vec![0u8; head + room * pointer].into_boxed_slice();
        let mut events = vec![super::blank_midi(); room].into_boxed_slice();
        for i in 0..room {
            let at = head + i * pointer;
            let address = &mut events[i] as *mut VstMidiEvent as usize;
            list[at..at + pointer].copy_from_slice(&address.to_ne_bytes());
        }
        Self { list, events }
    }

    /// The head with the block's MIDI events in, at their sample offsets.
    fn set(&mut self, midis: &[(u32, [u8; 4])]) -> *const VstEvents {
        let count = midis.len().min(self.events.len());
        let count = i32::try_from(count).unwrap_or(0);
        self.list[0..4].copy_from_slice(&count.to_ne_bytes());
        for (slot, (delta, bytes)) in self.events.iter_mut().zip(midis) {
            *slot = vestige::VstMidiEvent {
                r#type: vestige::kVstMidiType,
                byteSize: std::mem::size_of::<VstMidiEvent>() as i32,
                deltaSamples: i32::try_from(*delta).unwrap_or(0),
                flags: 0,
                noteLength: 0,
                noteOffset: 0,
                midiData: [
                    bytes[0] as std::ffi::c_char,
                    bytes[1] as std::ffi::c_char,
                    bytes[2] as std::ffi::c_char,
                    bytes[3] as std::ffi::c_char,
                ],
                detune: 0,
                noteOffVelocity: 0,
                reserved1: 0,
                reserved2: 0,
            };
        }
        self.list.as_ptr().cast()
    }
}

/// What the events that came with a block told about the song, for the flags the interface
/// names: whether a field was told at all, and whether the transport started or stopped.
#[derive(Default)]
struct Told {
    playing: Option<bool>,
    tempo: bool,
    beat: bool,
    bar: bool,
    cycle: bool,
    changed: bool,
}

/// The processing side of one instance, run by any of the process's audio threads, one at a
/// time ([`crate::audio`]): its buffers and its event list, and the plug-in's call fields as
/// they were copied at load.
struct Vst2Block {
    effect: *mut AEffect,
    dispatcher: Option<vestige::DispatcherFn>,
    process: Option<vestige::ProcessFn>,
    process_replacing: Option<vestige::ProcessFn>,
    set_parameter: Option<vestige::SetParameterFn>,
    flags: i32,
    shared: Arc<Shared>,
    max_frames: usize,
    link_channels: usize,
    buses: (usize, usize),
    in_buffers: Vec<Vec<f32>>,
    out_buffers: Vec<Vec<f32>>,
    /// The channel pointer arrays, built once and refilled per block without allocating.
    in_pointers: Vec<*mut f32>,
    out_pointers: Vec<*mut f32>,
    /// The block's MIDI events, gathered before the audio call; room for a block's worth.
    midis: Vec<(u32, [u8; 4])>,
    events: EventBuffer,
    time: Told,
}

impl Vst2Block {
    /// Sizes the buffers for the link and fills in the event pointers' targets, once.
    fn ready(mut self, served: &Served) -> Self {
        self.link_channels = served.channels() as usize;
        self.in_buffers = vec![vec![0.0; self.max_frames]; self.buses.0];
        self.out_buffers = vec![vec![0.0; self.max_frames]; self.buses.1];
        self.in_pointers = vec![std::ptr::null_mut(); self.buses.0];
        self.out_pointers = vec![std::ptr::null_mut(); self.buses.1];
        self.midis = Vec::with_capacity(1024);
        self
    }
}

// SAFETY: the block is run by one audio thread at a time (behind its lock, `crate::audio`),
// and its raw pointer and call fields are the plug-in's own, touched only within `process`
// and the cycle around the thread. Its buffers' heap memory does not move with it.
unsafe impl Send for Vst2Block {}

impl crate::audio::Block for Vst2Block {
    fn process(&mut self, input: &[f32], events: &[Event], output: &mut [f32]) {
        let frames = input.len().checked_div(self.link_channels).unwrap_or(0);
        // The song's place, and the events that become MIDI or parameter calls.
        self.midis.clear();
        for event in events {
            match event.kind {
                EventKind::Transport { playing, position } => {
                    self.time.changed |= self.time.playing.is_some_and(|was| was != playing);
                    self.time.playing = Some(playing);
                    self.shared.time.sample_pos.store((position as f64).to_bits(), Ordering::Relaxed);
                }
                EventKind::Tempo { bpm } => {
                    self.time.tempo = true;
                    self.shared.time.tempo.store(f64::from(bpm).to_bits(), Ordering::Relaxed);
                }
                EventKind::Beat { beat } => {
                    self.time.beat = true;
                    self.shared.time.ppq_pos.store(beat.to_bits(), Ordering::Relaxed);
                }
                EventKind::Bar { start, numerator, denominator, .. } => {
                    self.time.bar = true;
                    self.shared.time.bar_start.store(start.to_bits(), Ordering::Relaxed);
                    self.shared
                        .time
                        .time_sig
                        .store((i32::from(numerator) << 16) | i32::from(denominator), Ordering::Relaxed);
                }
                // The cycle in beats: VST2's own fields want PPQ positions (*assumed*).
                // The arrangement's loop in seconds is another kind of fact and faces no
                // PPQ on its way in; a second-based start in a field of beats would be
                // nonsense, so it is left for the formats that takes seconds.
                EventKind::Cycle { start, end, .. } => {
                    self.time.cycle = true;
                    self.shared.time.cycle_start.store(f64::from(start).to_bits(), Ordering::Relaxed);
                    self.shared.time.cycle_end.store(f64::from(end).to_bits(), Ordering::Relaxed);
                }
                // Seconds: not a VST2 field's own unit; noted no further.
                EventKind::CycleSeconds { .. } => {}
                // A parameter's value reaches the plug-in through `setParameter` before the
                // block's audio, at its frame's place in the order the engine sent (the
                // interface has no sample-accurate parameters; *assumed*).
                EventKind::Param { id, value } => {
                    if let (Some(set), Ok(index)) = (self.set_parameter, i32::try_from(id)) {
                        // SAFETY: the plug-in's own call, on this audio thread.
                        unsafe { set(self.effect, index, value) };
                    }
                }
                EventKind::NoteOn { channel, key, velocity } => {
                    self.midis.push((event.frame, [0x90 | channel, key, to_byte(velocity), 0]));
                }
                EventKind::NoteOff { channel, key, velocity } => {
                    self.midis.push((event.frame, [0x80 | channel, key, to_byte(velocity), 0]));
                }
                EventKind::Controller { channel, number, value } => {
                    self.midis.push((event.frame, [0xb0 | channel, number, to_byte(value), 0]));
                }
                EventKind::PitchBend { channel, value } => {
                    let bend = (8192.0 + value.clamp(-1.0, 1.0) * 8191.0).round() as i32;
                    self.midis.push((event.frame, [0xe0 | channel, (bend & 0x7f) as u8, (bend >> 7) as u8, 0]));
                }
                EventKind::Pressure { channel, value } => {
                    self.midis.push((event.frame, [0xd0 | channel, to_byte(value), 0, 0]));
                }
            }
        }
        // The flags of what this block told, with VeSTige's own bits.
        let mut flags = 0;
        if self.time.playing == Some(true) {
            flags |= vestige::kVstTransportPlaying;
        }
        if self.time.changed {
            flags |= vestige::kVstTransportChanged;
            self.time.changed = false;
        }
        if self.time.tempo {
            flags |= vestige::kVstTempoValid;
        }
        if self.time.beat {
            flags |= vestige::kVstPpqPosValid;
        }
        if self.time.bar {
            flags |= vestige::kVstBarsValid | vestige::kVstTimeSigValid;
        }
        if self.time.cycle {
            flags |= vestige::kVstCyclePosValid;
        }
        self.shared.time.flags.store(flags, Ordering::Relaxed);
        // The events, before the audio (the interface's one list for the block; *assumed*).
        if !self.midis.is_empty() {
            let list = self.events.set(&self.midis);
            if let Some(dispatcher) = self.dispatcher {
                // SAFETY: the plug-in's dispatcher with this block's event list, on this
                // audio thread; the list is this block's own buffer.
                unsafe {
                    dispatcher(
                        self.effect,
                        EffectOp::ProcessEvents.value(),
                        0,
                        0,
                        list.cast_mut().cast::<c_void>(),
                        0.0,
                    )
                };
            }
        }
        // The link's channels into the plug-in's inputs, and its outputs back.
        crate::audio::feed_main(input, self.link_channels, frames, &mut self.in_buffers);
        for channel in &mut self.out_buffers {
            channel.iter_mut().take(frames).for_each(|sample| *sample = 0.0);
        }
        for (slot, channel) in self.in_pointers.iter_mut().zip(&mut self.in_buffers) {
            *slot = channel.as_mut_ptr();
        }
        for (slot, channel) in self.out_pointers.iter_mut().zip(&mut self.out_buffers) {
            *slot = channel.as_mut_ptr();
        }
        // Say which instance this thread serves, so the plug-in's host calls answer it
        // without a lock; the process call, then the thread's word taken back.
        SERVING.with(|serving| {
            *serving.borrow_mut() = Some(self.shared.clone());
        });
        // `processReplacing` where the flag says so, the deprecated `process` otherwise (its
        // outputs add, so they were zeroed). A plug-in that declares the flag and gives no
        // replacing call falls back to `process` too: the zeroed outputs make it the same.
        let run = if self.flags & effect_flags::CAN_REPLACING != 0 {
            self.process_replacing.or(self.process)
        } else {
            self.process
        };
        if let Some(run) = run {
            let inputs = if self.in_pointers.is_empty() { std::ptr::null_mut() } else { self.in_pointers.as_mut_ptr() };
            // SAFETY: the plug-in's own call, with this block's buffers and frames, on this
            // audio thread.
            unsafe { run(self.effect, inputs, self.out_pointers.as_mut_ptr(), i32::try_from(frames).unwrap_or(0)) };
        }
        SERVING.with(|serving| {
            *serving.borrow_mut() = None;
        });
        crate::audio::collect_main(&self.out_buffers, self.link_channels, frames, output);
    }
}

/// A 0-to-1 value as the MIDI byte it travels as.
fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 127.0).round() as u8
}

/// The plug-in `spec` names, as the scanner's files are named (`vst2:<file>` or the built-in
/// mock).
fn open(spec: &str) -> Result<(Module, *mut AEffect), String> {
    let spec =
        if spec.starts_with("vst2:") || spec == super::probe::MOCK { spec.to_string() } else { format!("vst2:{spec}") };
    // The id a scan's spec carries (`vst2:<file>#<id>`): checked against what the file gives
    // back, so a spec naming one member of a shell never silently loads another. A suffix
    // that is not the eight-hex-digit id form is part of the file's name (`Synth#1.dll`),
    // not an id.
    let (spec, wanted) = match spec.rsplit_once('#') {
        Some((path, id)) if id.len() == 8 && id.chars().all(|c| c.is_ascii_hexdigit()) => {
            (path.to_string(), Some(id.to_string()))
        }
        _ => (spec.to_string(), None),
    };
    if spec == super::probe::MOCK {
        let (effect, mock) = super::test_vst2::mock(Some(host_callback));
        // The mock's own `AEffect`, whose `Mock` the module holds for as long as the effect
        // is called.
        let pointer = effect.as_ref() as *const AEffect as *mut AEffect;
        return Ok((Module::Mock(effect, mock), pointer));
    }
    let path = spec.strip_prefix("vst2:").ok_or_else(|| format!("not a VST2 plug-in name: {spec:?}"))?.to_string();
    let path = std::path::Path::new(&path);
    let library = load_library(path)?;
    let mut effect = std::ptr::null_mut();
    for name in ENTRY_NAMES {
        // SAFETY: the symbol the library exports under the assumed name, if it does.
        let symbol = unsafe { library.get::<Entry>(name.as_bytes()).map_err(|e| format!("{name}: {e}")) };
        let Ok(symbol) = symbol else { continue };
        // SAFETY: the entry of the plug-in, called with this host's callback as it is assumed
        // to take it; a plug-in that crashes here takes this disposable process.
        let answered = unsafe { symbol(Some(host_callback)) };
        effect = answered;
        if !answered.is_null() {
            break;
        }
    }
    if effect.is_null() {
        return Err(format!(
            "no entry of {} answered (tried, in order: {})",
            library_path_of(path).display(),
            ENTRY_NAMES.join(", ")
        ));
    }
    // The id a scan's spec carried, checked against what the file gave back.
    // SAFETY: the effect the entry gave, its id field, read on this thread.
    let id = unsafe { (*effect).uniqueID } as u32;
    if let Some(wanted) = wanted
        && wanted != format!("{id:08X}")
    {
        return Err(format!(
            "the plug-in {spec:?} gave back the effect {id:08X}, not {wanted} (a shell's member is not the file's first)"
        ));
    }
    Ok((Module::Library(library), effect))
}

/// The plug-ins the file `spec` names holds, without running any (for scanning): read its
/// identity, its counts and its category, then close it. A file's first effect is what is
/// listed; a shell's other members are an open question (the spec's).
pub(crate) fn describe(spec: &str) -> Result<Vec<suhina_plugins::scan::PluginInfo>, String> {
    let (_module, effect) = open(spec)?;
    // SAFETY: the effect the entry gave back, read by value once, on this thread.
    let read = unsafe { *effect };
    if read.magic != vestige::kEffectMagic {
        return Err(format!("the plug-in {spec:?} did not give a VST2 effect"));
    }
    let Some(dispatcher) = read.dispatcher else { return Err("the plug-in has no dispatcher".into()) };
    let ask = |op: EffectOp, index: i32, ptr: *mut c_void| -> isize {
        // SAFETY: the plug-in's dispatcher, with this call's arguments, on this thread,
        // before the plug-in is closed again.
        unsafe { dispatcher(effect, op.value(), index, 0, ptr, 0.0) }
    };
    // The plug-in is opened first, as a host does: a plug-in that fills its identity in
    // `effOpen` gives the scan nothing without it.
    let _ = ask(EffectOp::Open, 0, std::ptr::null_mut());
    // What it says about itself is what it became in `effOpen` too: counts, flags and the
    // id are read again, from the opened state (a plug-in that fills any of them in
    // `effOpen` gives the scan its real shape, not the blank it opened with).
    // SAFETY: the effect the entry gave back, read by value, on this thread, before the
    // plug-in is closed again.
    let read = unsafe { *effect };
    let mut name = vec![0 as std::ffi::c_char; vestige::VestigeMaxNameLen];
    let mut vendor = vec![0 as std::ffi::c_char; vestige::VestigeMaxLabelLen];
    let mut product = vec![0 as std::ffi::c_char; vestige::VestigeMaxLabelLen];
    // The buffers are this function's own, of the sizes the op-codes are assumed to give.
    let _ = ask(EffectOp::GetEffectName, 0, name.as_mut_ptr().cast::<c_void>());
    let _ = ask(EffectOp::GetVendorString, 0, vendor.as_mut_ptr().cast::<c_void>());
    let _ = ask(EffectOp::GetProductString, 0, product.as_mut_ptr().cast::<c_void>());
    let category = ask(EffectOp::GetPlugCategory, 0, std::ptr::null_mut()) as i32;
    let version = ask(EffectOp::GetVendorVersion, 0, std::ptr::null_mut());
    let _ = ask(EffectOp::Close, 0, std::ptr::null_mut());
    let counts = (usize::try_from(read.numInputs).unwrap_or(0), usize::try_from(read.numOutputs).unwrap_or(0));
    let instrument = category == vestige::kPlugCategSynth
        || category == vestige::kPlugCategGenerator
        || read.flags & vestige::effFlagsIsSynth != 0
        || counts.0 == 0 && counts.1 > 0;
    let name = super::field_text(&name);
    let vendor = super::field_text(&vendor);
    let product = super::field_text(&product);
    let path = PathBuf::from(spec.strip_prefix("vst2:").unwrap_or(spec));
    Ok(vec![suhina_plugins::scan::PluginInfo {
        path: path.clone(),
        id: format!("{:08X}", read.uniqueID as u32),
        name: if name.is_empty() { product } else { name },
        vendor,
        version: version.to_string(),
        features: vec![if instrument { "instrument".into() } else { "audio-effect".into() }],
        replaces: Vec::new(),
        ara: false,
    }])
}

/// The entry points a VST2 library is assumed to export, in the order the host tries them
/// (the probe reports which answered; *assumed*).
const ENTRY_NAMES: [&str; 3] = ["VSTPluginMain", "main_macho", "main"];

/// The library for the plug-in at `path`: the file itself, or in a macOS `.vst` bundle the
/// executable in `Contents/MacOS`, found as a VST3 bundle's is.
///
/// # Safety
///
/// Loading runs the file's initialisation code, which Rust cannot check; that is what this
/// process is for ([ADR 0006](../../../docs/decisions/0006-plug-in-hosting.md)). The library
/// stays loaded as long as the caller holds it, which must outlive every call on its effect.
pub(crate) fn load_library(path: &std::path::Path) -> Result<libloading::Library, String> {
    let file = library_path_of(path);
    // SAFETY: as the contract says; this is the one load, shared by the host and the probe.
    let library =
        unsafe { libloading::Library::new(&file) }.map_err(|e| format!("could not load {}: {e}", file.display()))?;
    Ok(library)
}

/// The executable that is loaded for the plug-in at `path`: the file itself, or in a bundle
/// the one the loader takes.
pub(crate) fn library_path_of(path: &std::path::Path) -> std::path::PathBuf {
    if path.is_file() || !cfg!(target_os = "macos") {
        return path.to_path_buf();
    }
    let Some(stem) = path.file_stem() else { return path.to_path_buf() };
    crate::vst3::macos_binary(&path.join("Contents/MacOS"), stem).unwrap_or_else(|| path.to_path_buf())
}

/// The signature every entry is assumed to have.
type Entry = unsafe extern "C" fn(vestige::AudioMasterCallback) -> *mut AEffect;

/// The host's tests, over a real link in this process.
#[cfg(test)]
#[path = "host_tests.rs"]
mod host_tests;
