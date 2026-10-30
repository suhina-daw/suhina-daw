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
    _value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    // The audio thread serves an instance now. The effect says who is asking; a call for the
    // instance this thread serves is answered from its lock-free slots, anything else by
    // looking the effect up.
    let serving = SERVING.with(|serving| serving.borrow().clone());
    let shared = if effect.is_null() {
        None
    } else {
        match &serving {
            Some(serving) if serving.effect == effect as usize => Some(serving.clone()),
            _ => lookup(effect),
        }
    };
    let Some(shared) = shared else { return 0 };
    let serving_this = serving.is_some_and(|s| Arc::ptr_eq(&s, &shared));
    answer(&shared, serving_this, opcode, index, ptr, opt)
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
fn answer(shared: &Shared, serving_this: bool, opcode: i32, index: i32, ptr: *mut c_void, opt: f32) -> isize {
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
            if serving_this {
                shared.notes.note(id, f64::from(opt));
            } else if let Ok(mut edits) = shared.edits.lock() {
                edits.push((id, opt));
            }
            0
        }
        // The gesture around a knob: noted, nothing to do.
        Some(HostOp::BeginEdit) | Some(HostOp::EndEdit) => 0,
        // No window of its own to size yet (editors are a later step of #1066).
        Some(HostOp::SizeWindow) => 0,
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
            // SAFETY: the caller's buffer, of the size the op-code is assumed to give.
            unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, "Suhina") };
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
    version: i32,
    sample_rate: f64,
    max_frames: usize,
    shared: Arc<Shared>,
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
        let count = |value: i32| usize::try_from(value).unwrap_or(0).min(MAX_CHANNELS);
        let shared = Arc::new(Shared {
            notes: Arc::new(crate::param_notes::ParamNotes::new()),
            edits: Mutex::new(Vec::new()),
            io_changed: AtomicBool::new(false),
            need_idle: AtomicBool::new(false),
            midi_out: AtomicU32::new(0),
            unique_id: read.uniqueID as u32,
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
            dispatcher: read.dispatcher,
            process: read.process,
            process_replacing: read.processReplacing,
            set_parameter: read.setParameter,
            get_parameter: read.getParameter,
            flags: read.flags,
            buses: (count(read.numInputs), count(read.numOutputs)),
            params: usize::try_from(read.numParams).unwrap_or(0).min(MAX_PARAMS),
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
    /// pinned header says nothing about a range), and the default is the value now, as the
    /// header names no op-code to ask for one.
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
            out.push(ParamInfo {
                id: u32::try_from(index).unwrap_or(0),
                name: super::field_text(&name),
                group: String::new(),
                min: 0.0,
                max: 1.0,
                default: value,
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
        Ok(ParamInfo {
            id,
            name: String::new(),
            group: String::new(),
            min: 0.0,
            max: 1.0,
            default: value,
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
            let size = usize::try_from(size).unwrap_or(0).min(MAX_CHUNK);
            let mut chunk = vec![0u8; size];
            if !pointer.is_null() {
                for (slot, i) in chunk.iter_mut().zip(0..size) {
                    let byte = pointer.cast::<u8>().wrapping_add(i);
                    // SAFETY: one of the `size` bytes the plug-in said its chunk has, capped.
                    *slot = unsafe { *byte };
                }
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
            for (index, value) in values.into_iter().enumerate().take(self.params) {
                self.set_value(i32::try_from(index).unwrap_or(0), value)?;
            }
            let _ = self.send(EffectOp::EndSetProgram, 0, 0, std::ptr::null_mut(), 0.0);
            return Ok(());
        }
        // Not a bank: a bare chunk, as a plug-in's own file would hold.
        if !data.is_empty() {
            let size = isize::try_from(data.len()).unwrap_or(0);
            let _ = self.send(EffectOp::SetChunk, 0, size, data.as_ptr().cast::<c_void>().cast_mut(), 0.0)?;
        }
        Ok(())
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
    // Resume, and open the process cycle, as a host does before the first block (*assumed*
    // order; the pinned header says only that the op-codes exist).
    // SAFETY: the plug-in's dispatcher, on this audio thread, before any block.
    unsafe { call(block.dispatcher, block.effect, EffectOp::MainsChanged, 1) };
    // SAFETY: as above.
    unsafe { call(block.dispatcher, block.effect, EffectOp::StartProcess, 0) };
    if let Some(block) = crate::audio::serve(instance, block, &mut served, stop) {
        // SAFETY: as above, after the last block.
        unsafe { call(block.dispatcher, block.effect, EffectOp::StopProcess, 0) };
        // SAFETY: as above.
        unsafe { call(block.dispatcher, block.effect, EffectOp::MainsChanged, 0) };
    }
    served
}

/// One dispatcher call from the audio thread, for the cycle around the blocks.
unsafe fn call(dispatcher: Option<vestige::DispatcherFn>, effect: *mut AEffect, op: EffectOp, value: isize) {
    let Some(dispatcher) = dispatcher else { return };
    // SAFETY: the plug-in's dispatcher with this call's arguments, on the thread that serves
    // its blocks.
    unsafe { dispatcher(effect, op.value(), 0, value, std::ptr::null_mut(), 0.0) };
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
                    self.shared.time.sample_pos.store(position, Ordering::Relaxed);
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
                EventKind::Cycle { start, end, .. } | EventKind::CycleSeconds { start, end } => {
                    self.time.cycle = true;
                    self.shared.time.cycle_start.store(f64::from(start).to_bits(), Ordering::Relaxed);
                    self.shared.time.cycle_end.store(f64::from(end).to_bits(), Ordering::Relaxed);
                }
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
        // outputs add, so they were zeroed).
        let run = if self.flags & effect_flags::CAN_REPLACING != 0 { self.process_replacing } else { self.process };
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
    // back, so a spec naming one member of a shell never silently loads another.
    let (spec, wanted) = match spec.rsplit_once('#') {
        Some((path, id)) => (path.to_string(), Some(id.to_string())),
        None => (spec, None),
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
    // A macOS bundle's executable is looked up as a VST3 bundle's is.
    let file = if path.is_file() {
        path.to_path_buf()
    } else if cfg!(target_os = "macos") {
        let stem = path.file_stem().ok_or("not a VST2 bundle")?;
        crate::vst3::macos_binary(&path.join("Contents/MacOS"), stem)
            .ok_or_else(|| format!("no executable in {}/Contents/MacOS", path.display()))?
    } else {
        return Err(format!("no VST2 plug-in file at {}", path.display()));
    };
    // SAFETY: loading the library runs its initialisation code, which Rust cannot check; that
    // is what this process is for (ADR 0006). The library stays loaded while the module holds
    // it, which outlives the effect.
    let library =
        unsafe { libloading::Library::new(&file) }.map_err(|e| format!("could not load {}: {e}", file.display()))?;
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
        return Err(format!("no entry of {} answered (tried, in order: {})", file.display(), ENTRY_NAMES.join(", ")));
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

/// The signature every entry is assumed to have.
type Entry = unsafe extern "C" fn(vestige::AudioMasterCallback) -> *mut AEffect;

/// The host's tests, over a real link in this process.
#[cfg(test)]
#[path = "host_tests.rs"]
mod host_tests;
