// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The probe: the host half of the VST2 interface, written as an observation tool first and
//! a host's base later (step 4 of #1066 builds the host on it). It loads a VST2 file — or the
//! built-in mock, `test:vst2` — runs it through the whole conversation a host has with a
//! plug-in, and records both directions: every call it made, with the answers, and every call
//! the plug-in made of it, with the answers it gave. What a real plug-in does with a fact the
//! pinned header lacks is what turns that fact from *assumed* into *observed* (ADR 0022
//! decision 4); the mock can only check that our two halves agree.
//!
//! The entry points of a VST2 library are the first such facts: the pinned header has none.
//! The probe tries `VSTPluginMain`, then `main_macho`, then `main` — the names, and the
//! signature (the audio-master callback in, the `AEffect` out), are assumed, and the report
//! says which one answered.
//!
//! The probe is a tool of the `suhina-plugin-host` process it runs in: a plug-in that crashes
//! mid-conversation takes the process with it, which is fine — `--vst2-probe` is run as a
//! disposable process, as the scanner is. Nothing here runs on an audio thread.

use std::ffi::c_char;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::test_vst2;
use super::vestige::{self, AEffect, VstTimeInfo};
use super::{
    Call, EffectInfo, EffectOp, EventList, HOST_VERSION, HostOp, MidiEvent, automation_state, c_str, can_do,
    effect_flags, field_text, process_level, read_events, write_text,
};

/// The name that loads the built-in mock plug-in.
pub(crate) const MOCK: &str = "test:vst2";
/// The sample rate the probe sets and answers with.
pub(crate) const SAMPLE_RATE: f64 = 48_000.0;
/// The block size the probe sets and answers with.
pub(crate) const BLOCK_SIZE: usize = 512;
/// Most parameters the probe reads (a plug-in's count is not trusted).
const MAX_PARAMS: usize = 64;
/// Most bytes the probe copies out of one chunk (a plug-in's size is not trusted; the copy
/// trusts the plug-in to have as many as it said, capped here).
const MAX_CHUNK: usize = 1024 * 1024;
/// Most events the probe takes from one list.
const MAX_EVENTS: usize = 1024;
/// Most entries each of the probe's records keeps: a plug-in can call back without limit
/// (a chatty one asked a vendor string hundreds of times in a second), so the newest are
/// kept and the report says when a record was cut.
const MAX_CALLS: usize = 4096;
/// Most channels a plug-in may declare before the probe refuses its block (the probe's
/// arrays are sized from the declaration, so the cap bounds them).
const MAX_CHANNELS: usize = 64;
/// The events the probe sends in its block: a note on and its off, a fifth and a quarter of
/// the block in.
pub(crate) const BLOCK_EVENTS: [([u8; 4], i32); 2] = [([0x90, 60, 100, 0], 100), ([0x80, 60, 64, 0], 200)];

/// The entry points a VST2 library is assumed to export, in the order the probe tries them.
/// *Assumed:* the pinned header has no entry point at all; the names, and the signature
/// (the host's callback in, the effect out), are this layer's choice, settled by running the
/// probe against installed plug-ins.
const ENTRY_NAMES: [&str; 3] = ["VSTPluginMain", "main_macho", "main"];

/// The signature every entry is assumed to have.
type Entry = unsafe extern "C" fn(vestige::AudioMasterCallback) -> *mut AEffect;

/// What the probe runs: the built-in mock, or a library and the effect its entry returned.
/// The library and the mock's state are held — and never read — to keep the plug-in's code
/// and its state alive for as long as the effect is called.
#[allow(dead_code)]
enum Probed {
    /// The mock's effect and the state its `user` field points at.
    Mock(Box<AEffect>, Arc<test_vst2::Mock>),
    /// The loaded library, the effect its entry returned, and which entry answered.
    Library(libloading::Library, *mut AEffect, &'static str),
}

/// The host's side: what it answers and everything the plug-in asked of it.
struct HostState {
    /// Every call the plug-in made, in order, with the answers the host gave.
    calls: Mutex<Vec<Call>>,
    /// Whether one of this probe's records was cut to its cap: the report's line about it,
    /// this probe's own and no other's.
    truncated: AtomicBool,
    /// The strings the plug-in asked `audioMasterCanDo` for, with the answers.
    can_do: Mutex<Vec<(String, isize)>>,
    /// The events the plug-in sent the host (its MIDI out).
    events: Mutex<Vec<MidiEvent>>,
    /// What the plug-in's counts were when it last said they changed, re-read then.
    io: Mutex<Option<EffectInfo>>,
    sample_rate: f64,
    block_size: usize,
    /// The time information `audioMasterGetTime` answers with. It lives at a stable address
    /// for as long as the probe runs: the answer is *assumed* to be a pointer to the host's
    /// `VstTimeInfo`, not a copy into a buffer the call would carry.
    time: Box<VstTimeInfo>,
}

impl HostState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            truncated: AtomicBool::new(false),
            can_do: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            io: Mutex::new(None),
            sample_rate: SAMPLE_RATE,
            block_size: BLOCK_SIZE,
            time: Box::new(VstTimeInfo {
                samplePos: 0.0,
                sampleRate: SAMPLE_RATE,
                nanoSeconds: 0.0,
                ppqPos: 0.0,
                tempo: 120.0,
                barStartPos: 0.0,
                cycleStartPos: 0.0,
                cycleEndPos: 0.0,
                timeSigNumerator: 4,
                timeSigDenominator: 4,
                smpteOffset: 0,
                smpteFrameRate: 0,
                samplesToNextClock: 0,
                flags: vestige::kVstTempoValid | vestige::kVstPpqPosValid | vestige::kVstTimeSigValid,
            }),
        })
    }

    /// Records one call of the plug-in, with the answer it was given.
    fn record(&self, call: Call) {
        if let Ok(mut calls) = self.calls.lock() {
            if bound(&mut calls) {
                self.truncated.store(true, Ordering::Release);
            }
            calls.push(call);
        }
    }
}

/// Keeps a record's newest entries when it is full; whether it cut, for the probe's report
/// line about it.
fn bound<T>(list: &mut Vec<T>) -> bool {
    if list.len() < MAX_CALLS {
        return false;
    }
    let cut = list.len() / 2;
    list.drain(..cut);
    true
}

/// The hosts of the probes this process runs, each under the effect it serves: the callback
/// finds its host as a real host would look an effect up in its own registry, and two probes
/// (the tests run theirs side by side) never answer each other's plug-ins. A call whose
/// effect is not one this process served — one made from inside a library's entry, before
/// the effect exists — is answered 0 and recorded nowhere; whether real plug-ins make such
/// calls is a question for the probe (the spec's open questions). A plug-in may call back
/// from any thread.
static REGISTRY: Mutex<Vec<(usize, Arc<HostState>)>> = Mutex::new(Vec::new());

/// The audio-master callback the probe hands the plug-in: records every call and answers as
/// a host would. Which argument carries what, and what an answer means, is *assumed* per
/// op-code (the spec lists each); the value of an answer is the host's own choice.
unsafe extern "C" fn host_callback(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    let key = effect as usize;
    let state = REGISTRY
        .lock()
        .ok()
        .and_then(|registry| registry.iter().find(|(effect, _)| *effect == key).map(|(_, state)| state.clone()));
    let Some(state) = state else { return 0 };
    let answer = answer(effect, opcode, index, value, ptr, opt, &state);
    state.record(Call { op: opcode, index, value, opt, answer: Some(answer) });
    answer
}

/// What the probe answers for one call of the plug-in.
fn answer(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
    state: &HostState,
) -> isize {
    let _ = (index, value, opt);
    match HostOp::from_value(opcode) {
        // The host's version. *Assumed* answer: 2400 stands for a 2.4 host.
        Some(HostOp::Version) => HOST_VERSION,
        // *Assumed:* the host answers the id of the plug-in being loaded.
        Some(HostOp::CurrentId) => {
            if effect.is_null() {
                return 0;
            }
            // SAFETY: one read of the caller's effect, its id field.
            unsafe { (*effect).uniqueID as isize }
        }
        // Noted; nothing to do.
        Some(HostOp::Idle)
        | Some(HostOp::WantMidi)
        | Some(HostOp::NeedIdle)
        | Some(HostOp::VendorSpecific)
        | Some(HostOp::UpdateDisplay) => 0,
        // *Assumed:* the answer is a pointer to the host's time information, which lives at
        // one address for as long as the probe runs.
        Some(HostOp::GetTime) => (&*state.time as *const VstTimeInfo).cast::<c_void>() as isize,
        // *Assumed:* `ptr` points at the plug-in's event list.
        Some(HostOp::ProcessEvents) => {
            // SAFETY: the list the plug-in sent, read with a bound.
            let events = unsafe { read_events(ptr.cast::<vestige::VstEvents>(), MAX_EVENTS) }.unwrap_or_default();
            if let Ok(mut sent) = state.events.lock() {
                if bound(&mut sent) {
                    state.truncated.store(true, Ordering::Release);
                }
                sent.extend(events);
            }
            0
        }
        // The tempo's unit in the answer is not observed; the probe declares none.
        Some(HostOp::TempoAt) => 0,
        // Noted: the counts are re-read for the report.
        Some(HostOp::IOChanged) => {
            // SAFETY: one read by value of the caller's effect.
            let info = unsafe { EffectInfo::of(effect) };
            if let Ok(mut io) = state.io.lock() {
                *io = info;
            }
            0
        }
        // No window, no latency of its own: the probe is not an audio device.
        Some(HostOp::SizeWindow) => 0,
        // *Assumed*: the answer is the rate as a whole number (the encoding of a float answer
        // is unobserved).
        Some(HostOp::GetSampleRate) => state.sample_rate as isize,
        Some(HostOp::GetBlockSize) => state.block_size as isize,
        Some(HostOp::GetInputLatency) | Some(HostOp::GetOutputLatency) => 0,
        // *Assumed* values; the probe runs its blocks as an offline render does.
        Some(HostOp::GetCurrentProcessLevel) => process_level::OFFLINE,
        // *Assumed* values; the probe automates nothing.
        Some(HostOp::GetAutomationState) => automation_state::OFF,
        // *Assumed*: the answer 1 says the string was filled.
        // The buffer's size is the op-code's assumption; writing only the string and its
        // terminator needs the least of it.
        Some(HostOp::GetVendorString) => {
            // SAFETY: the caller's buffer must hold the string and its NUL byte.
            unsafe { write_text(ptr, "Suhina".len() + 1, "Suhina") };
            1
        }
        Some(HostOp::GetProductString) => {
            // SAFETY: as above.
            unsafe { write_text(ptr, "Suhina VST2 probe".len() + 1, "Suhina VST2 probe") };
            1
        }
        Some(HostOp::GetVendorVersion) => 1,
        // *Assumed*: `ptr` names what is asked; 1 is yes. The strings the host says yes to
        // are assumed too.
        Some(HostOp::CanDo) => {
            // SAFETY: the plug-in's C string, read with a bound.
            let asked = unsafe { c_str(ptr, 128) }.unwrap_or_default();
            let yes =
                matches!(asked.as_str(), can_do::HOST_SENDS_EVENTS | can_do::HOST_SENDS_MIDI | can_do::HOST_SENDS_TIME);
            if let Ok(mut asked_for) = state.can_do.lock() {
                if bound(&mut asked_for) {
                    state.truncated.store(true, Ordering::Release);
                }
                asked_for.push((asked, isize::from(yes)));
            }
            isize::from(yes)
        }
        // From VeSTige: 1 is English.
        Some(HostOp::GetLanguage) => vestige::kVstLangEnglish as isize,
        Some(HostOp::BeginEdit) | Some(HostOp::EndEdit) => 0,
        // Anything else VeSTige names, and any op-code it does not: noted, not answered.
        _ => 0,
    }
}

/// The probe: one plug-in, run through the conversation a host has with it, recording both
/// directions into a report.
pub(crate) struct Probe {
    plug: Probed,
    state: Arc<HostState>,
    /// The report, a line at a time.
    report: Vec<String>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        let effect = self.effect() as usize;
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.retain(|(served, _)| *served != effect);
        }
    }
}

impl Probe {
    /// The probe over the built-in mock plug-in; the mock's records come back with it, so a
    /// test can read what the host sent.
    pub(crate) fn mock() -> (Self, Arc<test_vst2::Mock>) {
        let state = HostState::new();
        let (effect, mock) = test_vst2::mock(Some(host_callback));
        let mut probe = Self { plug: Probed::Mock(effect, mock.clone()), state, report: Vec::new() };
        let effect = probe.effect();
        Self::register(&probe.state, effect);
        (probe, mock)
    }

    /// A probe over a mock a test made itself, however it made it: for the cases the plain
    /// mock does not hold (a missing dispatcher, a wrong magic).
    #[cfg(test)]
    pub(super) fn over_mock(effect: Box<AEffect>, mock: Arc<test_vst2::Mock>) -> Self {
        let state = HostState::new();
        let mut probe = Self { plug: Probed::Mock(effect, mock), state, report: Vec::new() };
        let effect = probe.effect();
        Self::register(&probe.state, effect);
        probe
    }

    /// The probe over the VST2 file at `path`: the library is loaded, its entry called with
    /// the probe's callback, and the effect it returned checked for VeSTige's magic. The
    /// callback serves the effect from the registry once it exists; a call a plug-in makes
    /// from inside its entry, before the effect does, is answered 0 and recorded nowhere
    /// (see [`REGISTRY`]).
    pub(crate) fn file(path: &std::path::Path) -> Result<Self, String> {
        let state = HostState::new();
        // SAFETY: loading the library runs its initialisation code, which Rust cannot check;
        // that is what this disposable process is for (ADR 0006). The library stays loaded
        // while the probe holds it, which outlives every call on the effect.
        let library =
            unsafe { libloading::Library::new(path) }.map_err(|e| format!("could not load {}: {e}", path.display()))?;
        let mut answered_by = "";
        let mut effect = std::ptr::null_mut();
        for name in ENTRY_NAMES {
            // SAFETY: the symbol the library exports under the assumed name, if it does.
            let symbol = unsafe { library.get::<Entry>(name.as_bytes()).map_err(|e| format!("{name}: {e}")) };
            let Ok(symbol) = symbol else { continue };
            // SAFETY: the entry of the plug-in, called with the probe's callback as it is
            // assumed to take it; a plug-in that crashes here takes this process, which is
            // the probe's to lose.
            let answered = unsafe { symbol(Some(host_callback)) };
            answered_by = name;
            effect = answered;
            if !answered.is_null() {
                break;
            }
        }
        if effect.is_null() {
            return Err(format!(
                "no entry of {} answered (tried, in order: {})",
                path.display(),
                ENTRY_NAMES.join(", ")
            ));
        }
        // SAFETY: one field read of the effect the entry returned.
        let magic = unsafe { (*effect).magic };
        if magic != vestige::kEffectMagic {
            return Err(format!(
                "the effect {} gave back has the magic {magic:#x}, not VeSTige's {:#x}",
                path.display(),
                vestige::kEffectMagic
            ));
        }
        let mut probe = Self { plug: Probed::Library(library, effect, answered_by), state, report: Vec::new() };
        let effect = probe.effect();
        Self::register(&probe.state, effect);
        Ok(probe)
    }

    /// The host's state becomes the one the callback answers this effect's calls from.
    fn register(state: &Arc<HostState>, effect: *mut AEffect) {
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.retain(|(served, _)| *served != effect as usize);
            registry.push((effect as usize, state.clone()));
        }
    }

    /// The effect being probed.
    fn effect(&mut self) -> *mut AEffect {
        match &mut self.plug {
            Probed::Mock(effect, _) => effect.as_mut(),
            Probed::Library(_, effect, _) => *effect,
        }
    }

    /// One line of the report.
    fn line(&mut self, text: impl Into<String>) {
        self.report.push(text.into());
    }

    /// One dispatcher call to the plug-in. `Err` when it has no dispatcher (the call is not
    /// made, so the plug-in sees nothing).
    pub(super) fn send(
        &mut self,
        op: EffectOp,
        index: i32,
        value: isize,
        ptr: *mut c_void,
        opt: f32,
    ) -> Result<isize, String> {
        let effect = self.effect();
        // SAFETY: the effect the probe holds; one field read.
        let dispatcher = unsafe { (*effect).dispatcher };
        let Some(dispatcher) = dispatcher else { return Err(format!("{}: no dispatcher", op.name())) };
        // SAFETY: the dispatcher of the plug-in the probe holds, with this call's arguments;
        // a plug-in that crashes here takes this disposable process.
        Ok(unsafe { dispatcher(effect, op.value(), index, value, ptr, opt) })
    }

    /// One `getParameter` call.
    pub(super) fn parameter(&mut self, index: i32) -> Result<f32, String> {
        let effect = self.effect();
        // SAFETY: the effect the probe holds; one field read.
        let get = unsafe { (*effect).getParameter };
        let Some(get) = get else { return Err("the plug-in has no getParameter".into()) };
        // SAFETY: the plug-in's own call, with the effect it belongs to.
        Ok(unsafe { get(effect, index) })
    }

    /// One `setParameter` call.
    pub(super) fn set_parameter(&mut self, index: i32, value: f32) -> Result<(), String> {
        let effect = self.effect();
        // SAFETY: the effect the probe holds; one field read.
        let set = unsafe { (*effect).setParameter };
        let Some(set) = set else { return Err("the plug-in has no setParameter".into()) };
        // SAFETY: the plug-in's own call, with the effect it belongs to.
        unsafe { set(effect, index, value) };
        Ok(())
    }

    /// Runs the whole conversation and returns the report: what the plug-in is, what it
    /// answered, what it played, and everything it asked of the host. A step that cannot be
    /// made (no dispatcher, no `processReplacing`) ends its section with the reason; the
    /// conversation goes on, so one missing piece does not hide the rest.
    pub(crate) fn run(mut self) -> String {
        self.what_it_is();
        self.open();
        self.identify();
        self.parameters();
        self.state_chunks();
        self.block();
        self.close();
        self.host_calls();
        self.report.join("\n")
    }

    /// The report's head: the entry that answered (or the mock) and the effect's own fields.
    fn what_it_is(&mut self) {
        let effect = self.effect();
        // SAFETY: the effect the probe holds, read by value.
        let info = unsafe { EffectInfo::of(effect) };
        let Some(info) = info else {
            self.line("the plug-in gave no effect");
            return;
        };
        match &self.plug {
            Probed::Mock(..) => self.line(format!("plug-in: the built-in mock ({MOCK})")),
            Probed::Library(_, _, entry) => {
                self.line(format!("entry: {entry} (of the assumed names, in order: {})", ENTRY_NAMES.join(", ")));
            }
        }
        self.line(format!(
            "AEffect: {} programs, {} parameters, {} in, {} out, flags {:#x} (editor {}, processReplacing {}, synth {}, chunks {} (assumed bit))",
            info.num_programs,
            info.num_params,
            info.num_inputs,
            info.num_outputs,
            info.flags,
            yes_no(info.flags & effect_flags::HAS_EDITOR != 0),
            yes_no(info.flags & effect_flags::CAN_REPLACING != 0),
            yes_no(info.flags & effect_flags::IS_SYNTH != 0),
            yes_no(info.flags & effect_flags::HAS_CHUNKS != 0),
        ));
        self.line(format!(
            "  id {}, version {}, VeSTige's \"1.0f\" field holds {}, its twelve zero bytes hold {:?} (the pinned header names no latency field)",
            info.unique_id, info.version, info.unknown_float, info.empty3
        ));
        // What the effect actually holds, against what its flag declares: a plug-in can
        // declare `processReplacing` and give no call, which the report makes plain.
        self.line(format!(
            "  calls: dispatcher {}, process {}, processReplacing {} (the flag says {})",
            yes_no(info.has_dispatcher),
            yes_no(info.has_process),
            yes_no(info.has_process_replacing),
            yes_no(info.flags & effect_flags::CAN_REPLACING != 0),
        ));
        if !info.has_dispatcher {
            self.line("  no dispatcher: the conversation cannot begin");
        }
    }

    /// Opens the plug-in and sets its sizes, as a host does before anything else.
    fn open(&mut self) {
        self.line("open (the rate in the call's float and the size in its value are assumed):");
        for (op, value, opt) in [
            (EffectOp::Open, 0, 0.0),
            (EffectOp::SetSampleRate, 0, SAMPLE_RATE as f32),
            (EffectOp::SetBlockSize, BLOCK_SIZE as isize, 0.0),
        ] {
            match self.send(op, 0, value, std::ptr::null_mut(), opt) {
                Ok(_) => self.line(format!("  {}", op.name())),
                Err(e) => self.line(format!("  {e}")),
            }
        }
    }

    /// What the plug-in says it is.
    fn identify(&mut self) {
        self.line("what it says it is:");
        for (op, what) in [
            (EffectOp::GetVstVersion, "2400 assumed to mean 2.4"),
            (EffectOp::GetPlugCategory, "VeSTige's category numbers"),
        ] {
            match self.send(op, 0, 0, std::ptr::null_mut(), 0.0) {
                Ok(answer) => self.line(format!("  {} -> {answer} ({what})", op.name())),
                Err(e) => self.line(format!("  {e}")),
            }
        }
        for (op, room) in [
            (EffectOp::GetEffectName, vestige::VestigeMaxNameLen),
            (EffectOp::GetVendorString, vestige::VestigeMaxLabelLen),
            (EffectOp::GetProductString, vestige::VestigeMaxLabelLen),
        ] {
            let mut field = vec![0 as c_char; room];
            match self.send(op, 0, 0, field.as_mut_ptr().cast::<c_void>(), 0.0) {
                Ok(_) => self.line(format!("  {} -> \"{}\"", op.name(), field_text(&field))),
                Err(e) => self.line(format!("  {e}")),
            }
        }
        match self.send(EffectOp::GetVendorVersion, 0, 0, std::ptr::null_mut(), 0.0) {
            Ok(answer) => self.line(format!("  effGetVendorVersion -> {answer}")),
            Err(e) => self.line(format!("  {e}")),
        }
        // The host asks the plug-in what it can do, with the assumed strings.
        for asked in [can_do::ASK_RECEIVES_EVENTS, can_do::ASK_RECEIVES_MIDI, can_do::ASK_RECEIVES_TIME] {
            let name = super::c_name(asked);
            match self.send(EffectOp::CanDo, 0, 0, name.as_ptr().cast::<c_void>().cast_mut(), 0.0) {
                Ok(answer) => self.line(format!("  effCanDo \"{asked}\" -> {answer} (1 assumed to be yes)")),
                Err(e) => self.line(format!("  {e}")),
            }
        }
    }

    /// The plug-in's parameters: names, units, displayed values and values.
    fn parameters(&mut self) {
        // SAFETY: the effect the probe holds, read by value.
        let Some(info) = (unsafe { EffectInfo::of(self.effect()) }) else { return };
        let declared = usize::try_from(info.num_params).unwrap_or(0);
        let count = declared.min(MAX_PARAMS);
        if declared > MAX_PARAMS {
            self.line(format!(
                "parameters: {} declared, {} read (a plug-in's count is not trusted)",
                info.num_params, count
            ));
        } else {
            self.line(format!("parameters: {count}"));
        }
        for index in 0..count {
            let index = i32::try_from(index).unwrap_or(0);
            let mut name = vec![0 as c_char; vestige::VestigeMaxLabelLen];
            let mut unit = vec![0 as c_char; vestige::VestigeMaxLabelLen];
            let mut display = vec![0 as c_char; vestige::VestigeMaxLabelLen];
            let asked = self.send(EffectOp::GetParamName, index, 0, name.as_mut_ptr().cast::<c_void>(), 0.0);
            let _ = self.send(EffectOp::GetParamLabel, index, 0, unit.as_mut_ptr().cast::<c_void>(), 0.0);
            let _ = self.send(EffectOp::GetParamDisplay, index, 0, display.as_mut_ptr().cast::<c_void>(), 0.0);
            let value = self.parameter(index);
            let (unit, display) = (field_text(&unit), field_text(&display));
            match (asked, value) {
                (Ok(_), Ok(value)) => {
                    self.line(format!("  {index}: \"{}\" (\"{unit}\", \"{display}\") = {value:.6}", field_text(&name)))
                }
                (Err(e), _) => self.line(format!("  {e}")),
                (_, Err(e)) => self.line(format!("  {e}")),
            }
        }
        // A host sets a parameter before it plays: the call is not a dispatcher call but the
        // effect's own, and the probe exercises it once.
        if count > 0
            && let Err(e) = self.set_parameter(0, 0.25)
        {
            self.line(format!("  {e}"));
        }
    }

    /// The plug-in's state as a chunk: asked for, handed back, and the value asked again
    /// after — the round trip a project's saved settings depend on.
    fn state_chunks(&mut self) {
        self.line("state as a chunk (the op-codes, and what `index` and the answer mean, are assumed):");
        let mut chunk_pointer: *const c_void = std::ptr::null();
        let claimed =
            match self.send(EffectOp::GetChunk, 0, 0, &mut chunk_pointer as *mut *const c_void as *mut c_void, 0.0) {
                Ok(size) => usize::try_from(size).unwrap_or(0),
                Err(e) => {
                    self.line(format!("  {e}"));
                    return;
                }
            };
        // A size without the pointer it says it has is not a state to round-trip: the report
        // says so instead of handing fabricated zero bytes back.
        if claimed > 0 && chunk_pointer.is_null() {
            self.line(format!(
                "  {} -> {claimed} bytes claimed, but no chunk pointer came with them",
                EffectOp::GetChunk.name()
            ));
            return;
        }
        // Over the cap the copy would not be the plug-in's whole state: the round trip of a
        // truncated chunk would read as evidence it round-trips when it has not.
        if claimed > MAX_CHUNK {
            self.line(format!(
                "  {} -> {claimed} bytes, over the {MAX_CHUNK}-byte cap: the round trip is skipped, the report shows the head only",
                EffectOp::GetChunk.name()
            ));
            let head: Vec<String> = (0..8)
                .map(|i| {
                    let byte = chunk_pointer.cast::<u8>().wrapping_add(i);
                    // SAFETY: the head bytes of the chunk the plug-in pointed at, at most
                    // the cap away.
                    format!("{:02x}", unsafe { *byte })
                })
                .collect();
            self.line(format!("  head: {}", head.join(" ")));
            return;
        }
        let size = claimed;
        let mut chunk = vec![0u8; size];
        for (slot, i) in chunk.iter_mut().zip(0..size) {
            let byte = chunk_pointer.cast::<u8>().wrapping_add(i);
            // SAFETY: one of the `size` bytes the plug-in said its chunk has, at the
            // pointer it gave.
            *slot = unsafe { *byte };
        }
        let head: Vec<String> = chunk.iter().take(8).map(|b| format!("{b:02x}")).collect();
        self.line(format!("  {} -> {size} bytes: {}", EffectOp::GetChunk.name(), head.join(" ")));
        // Hand the chunk back and see whether what it holds came back: the mock's chunk is
        // its gain.
        let before = self.parameter(0).ok();
        if let Err(e) = self.set_chunk(&chunk) {
            self.line(format!("  {e}"));
            return;
        }
        let after = self.parameter(0).ok();
        self.line(format!("  {} handed it back: parameter 0 was {before:?}, is {after:?}", EffectOp::SetChunk.name()));
    }

    /// Hands the plug-in a chunk.
    fn set_chunk(&mut self, chunk: &[u8]) -> Result<(), String> {
        let size = isize::try_from(chunk.len()).unwrap_or(0);
        self.send(EffectOp::SetChunk, 0, size, chunk.as_ptr().cast::<c_void>().cast_mut(), 0.0).map(|_| ())
    }

    /// One block, with events: resume, start, play, idle, stop, suspend — the cycle a host
    /// runs — and what came out of it.
    fn block(&mut self) {
        self.line("one block (the cycle and its meanings assumed where marked):");
        for (op, value, what) in [
            (EffectOp::MainsChanged, 1, "resume, non-zero assumed to resume"),
            (EffectOp::StartProcess, 0, "the process cycle opens"),
        ] {
            match self.send(op, 0, value, std::ptr::null_mut(), 0.0) {
                Ok(_) => self.line(format!("  {} ({what})", op.name())),
                Err(e) => self.line(format!("  {e}")),
            }
        }
        // The events of the block, handed over before it runs.
        let mut events = EventList::<4>::new(&BLOCK_EVENTS);
        // SAFETY: the list is pointed at its own events, which stay where they are for the
        // call.
        let list = events.ready();
        match self.send(EffectOp::ProcessEvents, 0, 0, list.cast_mut().cast::<c_void>(), 0.0) {
            Ok(answer) => self.line(format!("  effProcessEvents (2 events) -> {answer}")),
            Err(e) => self.line(format!("  {e}")),
        }
        // The block itself: `processReplacing` where the flag says so, the deprecated
        // `process` otherwise. The channels are what the plug-in declared (a plug-in reads
        // its own count against the arrays it is handed), capped; one over the cap is
        // refused the block rather than read past the arrays.
        // SAFETY: the effect the probe holds, read by value.
        let info = unsafe { EffectInfo::of(self.effect()) };
        let replacing = info.is_some_and(|info| info.flags & effect_flags::CAN_REPLACING != 0);
        let (ins, outs_declared) = info
            .map(|i| (usize::try_from(i.num_inputs).unwrap_or(0), usize::try_from(i.num_outputs).unwrap_or(0)))
            .unwrap_or((2, 2));
        if ins > MAX_CHANNELS || outs_declared > MAX_CHANNELS {
            self.line(format!(
                "  the block is refused: the plug-in declares {ins} in and {outs_declared} out, over the {MAX_CHANNELS}-channel cap"
            ));
            return;
        }
        let frames = BLOCK_SIZE;
        let channels: Vec<Vec<f32>> = (0..ins.max(1)).map(|_| (0..frames).map(ramp).collect()).collect();
        let mut outs: Vec<Vec<f32>> = (0..outs_declared.max(1)).map(|_| vec![0.0; frames]).collect();
        let mut in_pointers: Vec<*mut f32> = channels.iter().map(|c| c.as_ptr() as *mut f32).collect();
        let mut out_pointers: Vec<*mut f32> = outs.iter_mut().map(|c| c.as_mut_ptr()).collect();
        let effect = self.effect();
        let sent = if replacing {
            // SAFETY: the effect the probe holds; one field read.
            let process = unsafe { (*effect).processReplacing };
            process.map(|process| {
                // SAFETY: the plug-in's own call, with the buffers and frames given.
                unsafe { process(effect, in_pointers.as_mut_ptr(), out_pointers.as_mut_ptr(), frames as i32) };
                "processReplacing"
            })
        } else {
            // SAFETY: as above, for the deprecated call.
            let process = unsafe { (*effect).process };
            process.map(|process| {
                // SAFETY: the plug-in's own call, with the buffers and frames given.
                unsafe { process(effect, in_pointers.as_mut_ptr(), out_pointers.as_mut_ptr(), frames as i32) };
                "process (deprecated; the flag said no replacing)"
            })
        };
        match sent {
            Some(what) => self.line(format!(
                "  {what} of {frames} frames ({ins} in, {outs_declared} out): in[0][0] {:.3} -> out[0][0] {:.3}, out[0][last] {:.3}",
                channels[0].first().copied().unwrap_or_default(),
                outs[0].first().copied().unwrap_or_default(),
                outs[0].last().copied().unwrap_or_default(),
            )),
            None => self.line("  the plug-in has no process call"),
        }
        // An editor's idle, where a plug-in with a panel reports the user's moves: only a
        // plug-in that declares an editor gets one (a call without an editor open is one
        // some plug-ins take badly).
        if info.is_some_and(|info| info.flags & effect_flags::HAS_EDITOR != 0)
            && let Err(e) = self.send(EffectOp::EditIdle, 0, 0, std::ptr::null_mut(), 0.0)
        {
            self.line(format!("  {e}"));
        }
        for (op, value, what) in
            [(EffectOp::StopProcess, 0, "the process cycle closes"), (EffectOp::MainsChanged, 0, "suspend")]
        {
            match self.send(op, 0, value, std::ptr::null_mut(), 0.0) {
                Ok(_) => self.line(format!("  {} ({what})", op.name())),
                Err(e) => self.line(format!("  {e}")),
            }
        }
    }

    /// Closes the plug-in.
    fn close(&mut self) {
        match self.send(EffectOp::Close, 0, 0, std::ptr::null_mut(), 0.0) {
            Ok(_) => self.line("effClose"),
            Err(e) => self.line(e),
        }
    }

    /// Everything the plug-in asked of the host, with the answers it was given: the half of
    /// the conversation only a real plug-in can teach us.
    fn host_calls(&mut self) {
        self.line("the calls the plug-in made of its host (with the host's answers):");
        let calls = self.state.calls.lock().map(|calls| calls.clone()).unwrap_or_default();
        if calls.is_empty() {
            self.line("  none");
        }
        for call in calls {
            let name =
                HostOp::from_value(call.op).map_or_else(|| format!("op-code {}", call.op), |op| op.name().to_owned());
            let answer = call.answer.map_or_else(String::new, |a| format!(" -> {a}"));
            self.line(format!("  {name}(index {}, value {}, opt {:.6}){answer}", call.index, call.value, call.opt));
        }
        if self.state.truncated.load(Ordering::Acquire) {
            self.line("  (some records were cut to the newest 4096 entries: a chatty plug-in)");
        }
        let asked = self.state.can_do.lock().map(|asked| asked.clone()).unwrap_or_default();
        for (what, answer) in asked {
            self.line(format!("  audioMasterCanDo \"{what}\" -> {answer} (1 assumed to be yes)"));
        }
        let events = self.state.events.lock().map(|events| events.clone()).unwrap_or_default();
        if !events.is_empty() {
            let shown: Vec<String> = events.iter().take(8).map(MidiEvent::text).collect();
            self.line(format!("  events the plug-in sent the host: {}", shown.join(", ")));
        }
        let changed = self.state.io.lock().ok().and_then(|io| *io);
        if let Some(info) = changed {
            self.line(format!("  after audioMasterIOChanged: {} in, {} out", info.num_inputs, info.num_outputs));
        }
    }
}

/// The signal the probe feeds the plug-in: a ramp, so a gain shows in every sample.
fn ramp(frame: usize) -> f32 {
    (frame % 24) as f32 / 24.0
}

/// `yes` or `no`, for the report.
fn yes_no(yes: bool) -> &'static str {
    if yes { "yes" } else { "no" }
}

impl MidiEvent {
    /// One event as a report line.
    fn text(&self) -> String {
        let bytes: Vec<String> = self.data.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}@{} [{}]",
            if self.kind == vestige::kVstMidiType { "midi".to_string() } else { format!("type {}", self.kind) },
            self.delta_samples,
            bytes.join(" ")
        )
    }
}

/// `--vst2-probe <name>`: runs the probe over the VST2 file `name` names, or over the built-in
/// mock when `name` is [`MOCK`], and returns its report. `Err` is what could not even be
/// loaded.
pub(crate) fn run_tool(name: &str) -> Result<String, String> {
    let probe = if name == MOCK {
        let (probe, _mock) = Probe::mock();
        probe
    } else {
        Probe::file(std::path::Path::new(name))?
    };
    Ok(probe.run())
}
