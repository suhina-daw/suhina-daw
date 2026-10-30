// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A VST2 plug-in built into the host for tests and the probe (`test:vst2`), standing where
//! [`test_vst3`](crate::test_vst3) stands for VST3: a gain (0.5 at first) with its state (the
//! gain as 4 bytes, little-endian `f32`, the same shape as `test:vst3`'s), two channels in
//! and out, and one parameter. It records every call the host makes — every dispatcher call
//! with its arguments, every `setParameter`, every block, every event list — and it makes the
//! calls a plug-in makes of its host: it asks the host's version and what it can do when it
//! opens, asks the process level and the time while it processes, and, on the first
//! `effEditIdle`, turns its gain through `audioMasterBeginEdit`/`audioMasterAutomate`/
//! `audioMasterEndEdit` as a user turning a knob would (it has no editor of its own; editors
//! come with the host's window work, step 7 of #1066).
//!
//! What the mock answers is written from this layer's *assumptions*, so a mock-and-probe pair
//! proves only that the two halves of our own code agree; an *observed* fact needs the probe
//! against an installed plug-in (ADR 0022 decision 4). The assumptions are marked where they
//! are made and listed in `docs/spec/plugin-vst2.md`.
//!
//! The mock's `AEffect` keeps its state the way a real plug-in keeps its own: the `user`
//! field points at the [`Mock`] the caller holds for as long as the effect lives. That is the
//! mock's own business; a host must never write a plug-in's fields.

use std::ffi::c_char;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::Mutex;

use super::vestige::{self, AEffect, VstEvents, VstParameterProperties};
use super::{Call, EffectOp, HostOp, MidiEvent, can_do, effect_flags, fill_field, read_events};

/// The mock's name, as `effGetEffectName` answers.
pub(crate) const NAME: &str = "Suhina Test Gain";
/// The gain the mock turns to on the first `effEditIdle`, as `test:vst3`'s editor does.
pub(crate) const EDIT_GAIN: f32 = 0.125;
/// Most events one event list may hold for the mock (a host's list is not trusted either).
pub(crate) const MAX_EVENTS: usize = 1024;
/// Most bytes one chunk may hold for the mock.
pub(crate) const MAX_CHUNK: usize = 1024 * 1024;
/// The channels the mock declared, in and out.
pub(crate) const CHANNELS: usize = 2;
/// The mock's unique id, four characters packed as VeSTige's `CCONST` does it.
pub(crate) const UNIQUE_ID: i32 = vestige::CCONST(b'S', b'h', b'T', b'G');
/// The latency the mock reports, in the field the host assumes holds it: the first of
/// VeSTige's twelve "zero" bytes.
pub(crate) const LATENCY: i32 = 32;
/// Most entries each of the mock's records keeps: the mock runs for a whole session in the
/// host's tests, so its records cannot grow without end — the newest are kept.
const MAX_RECORDED: usize = 1024;

/// The mock's editor rectangle as it writes it, the shape the op-code is assumed to fill.
#[repr(C)]
struct EditorRect {
    top: i16,
    left: i16,
    bottom: i16,
    right: i16,
}

/// The mock's editor: 300 × 200, as `test:vst3`'s is; the rect `effEditGetRect` names.
static MOCK_EDITOR_RECT: EditorRect = EditorRect { top: 0, left: 0, bottom: 200, right: 300 };

/// What the mock has seen and what it holds, all behind one lock. The records are bounded
/// ([`MAX_RECORDED`]): the mock runs for a whole session through the host, and its records
/// are its product for the tests, not a log without end.
#[derive(Default)]
struct Inner {
    /// The host callback the mock was given, as an entry point hands it over.
    host: vestige::AudioMasterCallback,
    /// Every dispatcher call the host made, in order, with the answer the mock gave.
    calls: Vec<Call>,
    /// Every call the mock made of its host, in order, with the answer the host gave.
    asks: Vec<Call>,
    /// Every `setParameter`, in order.
    parameters: Vec<(i32, f32)>,
    /// Every `getParameter`, in order.
    reads: Vec<i32>,
    /// Blocks processed: (frames, how many events the lists before the block brought).
    blocks: Vec<(usize, usize)>,
    gain: f32,
    sample_rate: f64,
    block_size: usize,
    /// Whether `effMainsChanged` last resumed the mock.
    resumed: bool,
    /// Whether `effStartProcess` was answered and `effStopProcess` has not come.
    processing: bool,
    program: i32,
    /// The event lists `effProcessEvents` brought, the events of each in order.
    events: Vec<Vec<MidiEvent>>,
    /// The events of the last list, waiting for the block they belong to; taken by it, so
    /// they play once.
    pending: Vec<MidiEvent>,
    /// The chunk `effGetChunk` hands out and `effSetChunk` last brought.
    chunk: Vec<u8>,
    /// Whether the first `effEditIdle` has turned the gain.
    automated: bool,
    /// Whether an editor window's handle was handed over, and the handle as an address.
    editor_open: bool,
    editor_handle: usize,
    /// The sample rate the mock read out of the host's time information, if it asked.
    time_sample_rate: Option<f64>,
}

impl Inner {
    /// The gain the mock starts with, so the default is visible here and not only in tests.
    fn with_defaults() -> Self {
        Self { gain: 0.5, ..Self::default() }
    }
}

/// The mock plug-in's state, shared with the probe that runs it.
pub(crate) struct Mock(Mutex<Inner>);

/// The mock's `AEffect`, and the state its `user` field points at: the caller keeps both for
/// as long as the effect is called, and hands the effect to the probe as a loaded plug-in.
/// `host` is the audio-master callback a library's entry point would be given.
pub(crate) fn mock(host: vestige::AudioMasterCallback) -> (Box<AEffect>, Arc<Mock>) {
    let mock = Arc::new(Mock(Mutex::new(Inner::with_defaults())));
    if let Ok(mut inner) = mock.0.lock() {
        inner.host = host;
    }
    // The latency, in the field the host assumes holds it (the pinned header names none).
    let mut empty3 = [0 as c_char; 12];
    for (slot, byte) in empty3.iter_mut().zip(LATENCY.to_ne_bytes()) {
        *slot = byte as c_char;
    }
    let effect = Box::new(AEffect {
        magic: vestige::kEffectMagic,
        dispatcher: Some(dispatcher),
        process: Some(process),
        setParameter: Some(set_parameter),
        getParameter: Some(get_parameter),
        numPrograms: 1,
        numParams: 1,
        numInputs: CHANNELS as i32,
        numOutputs: CHANNELS as i32,
        flags: effect_flags::CAN_REPLACING | effect_flags::HAS_CHUNKS | effect_flags::HAS_EDITOR,
        ptr1: std::ptr::null_mut(),
        ptr2: std::ptr::null_mut(),
        empty3,
        unkown_float: 1.0,
        ptr3: std::ptr::null_mut(),
        user: Arc::as_ptr(&mock).cast_mut().cast(),
        uniqueID: UNIQUE_ID,
        version: 1,
        processReplacing: Some(process_replacing),
    });
    (effect, mock)
}

/// The mock's records are its product: the tests read what the host sent through these
/// (the probe prints its own half of the conversation), so in the host process itself they
/// are what keeps the mock honest for the tests that compile against it.
#[allow(dead_code)]
impl Mock {
    /// Every dispatcher call the host made, in order, with the mock's answers.
    pub(crate) fn calls(&self) -> Vec<Call> {
        self.0.lock().map(|inner| inner.calls.clone()).unwrap_or_default()
    }

    /// Every call the mock made of its host, in order, with the host's answers.
    pub(crate) fn asks(&self) -> Vec<Call> {
        self.0.lock().map(|inner| inner.asks.clone()).unwrap_or_default()
    }

    /// Every `setParameter` the host made, in order.
    pub(crate) fn parameters(&self) -> Vec<(i32, f32)> {
        self.0.lock().map(|inner| inner.parameters.clone()).unwrap_or_default()
    }

    /// Every block the host ran, as (frames, events the lists before it brought).
    pub(crate) fn blocks(&self) -> Vec<(usize, usize)> {
        self.0.lock().map(|inner| inner.blocks.clone()).unwrap_or_default()
    }

    /// The event lists `effProcessEvents` brought.
    pub(crate) fn events(&self) -> Vec<Vec<MidiEvent>> {
        self.0.lock().map(|inner| inner.events.clone()).unwrap_or_default()
    }

    /// The gain the mock holds now.
    pub(crate) fn gain(&self) -> f32 {
        self.0.lock().map(|inner| inner.gain).unwrap_or_default()
    }

    /// The sample rate the host last set, as the mock read it from `effSetSampleRate`.
    pub(crate) fn sample_rate(&self) -> f64 {
        self.0.lock().map(|inner| inner.sample_rate).unwrap_or_default()
    }

    /// The block size the host last set, as the mock read it from `effSetBlockSize`.
    pub(crate) fn block_size(&self) -> usize {
        self.0.lock().map(|inner| inner.block_size).unwrap_or_default()
    }

    /// Whether `effMainsChanged` last resumed the mock, and whether a process cycle is open.
    pub(crate) fn resumed_and_processing(&self) -> (bool, bool) {
        self.0.lock().map(|inner| (inner.resumed, inner.processing)).unwrap_or((false, false))
    }

    /// The sample rate the mock read out of the host's time information, when it asked.
    pub(crate) fn time_sample_rate(&self) -> Option<f64> {
        self.0.lock().ok().and_then(|inner| inner.time_sample_rate)
    }

    /// The chunk the mock last saved or was given.
    pub(crate) fn chunk(&self) -> Vec<u8> {
        self.0.lock().map(|inner| inner.chunk.clone()).unwrap_or_default()
    }

    /// Whether an editor window's handle arrived, and the handle, from `effEditOpen`.
    pub(crate) fn editor(&self) -> (bool, usize) {
        self.0.lock().map(|inner| (inner.editor_open, inner.editor_handle)).unwrap_or((false, 0))
    }

    /// Records one call the host made.
    fn saw(&self, call: Call) {
        if let Ok(mut inner) = self.0.lock() {
            bound(&mut inner.calls);
            inner.calls.push(call);
        }
    }

    /// Asks the host through the callback it gave the mock, records the call with the answer,
    /// and returns the answer.
    fn ask(
        &self,
        effect: *mut AEffect,
        op: HostOp,
        index: i32,
        value: isize,
        ptr: *mut c_void,
        opt: f32,
    ) -> Option<isize> {
        let host = self.0.lock().ok().and_then(|inner| inner.host);
        let answer = host.map(|callback| {
            // SAFETY: the callback the host itself gave the mock, called with this effect's
            // pointer; the host vouches for its own callback.
            unsafe { callback(effect, op.value(), index, value, ptr, opt) }
        });
        if let Ok(mut inner) = self.0.lock() {
            bound(&mut inner.asks);
            inner.asks.push(Call { op: op.value(), index, value, opt, answer });
        }
        answer
    }
}

/// Keeps a record's newest entries when it is full: the mock runs for a whole session
/// through the host, and its records are its product for the tests, not a log without end.
fn bound<T>(list: &mut Vec<T>) {
    if list.len() >= MAX_RECORDED {
        let cut = list.len() / 2;
        list.drain(..cut);
    }
}

/// The mock an `AEffect` belongs to, or `None` when the effect is not the mock's (its `user`
/// field was not set by [`mock`]).
///
/// # Safety
///
/// The effect must be one [`mock`] returned, whose [`Mock`] the caller still holds.
unsafe fn mock_of(effect: *mut AEffect) -> Option<&'static Mock> {
    if effect.is_null() {
        return None;
    }
    // SAFETY: one read of the mock's own struct, whose lifetime the caller vouches for.
    let user = unsafe { (*effect).user };
    if user.is_null() {
        return None;
    }
    // SAFETY: the pointer `mock()` put in `user`: the `Mock` the caller holds, which outlives
    // every call on the effect.
    Some(unsafe { &*(user.cast::<Mock>()) })
}

/// The mock's dispatcher: everything a host asks except audio and parameter values. The
/// answers come from this layer's assumptions, marked in the spec; every call is recorded
/// with its answer, whatever the host asked for.
unsafe extern "C" fn dispatcher(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    // SAFETY: the effect is the mock's (the probe or a test holds it and its `Mock`).
    let Some(mock) = (unsafe { mock_of(effect) }) else { return 0 };
    let answer = match EffectOp::from_value(opcode) {
        Some(EffectOp::Open) => {
            // A plug-in meets its host as it opens: the version it is built for (assumed
            // 2400), what the host can do (assumed strings), and the size of its blocks.
            let _ = mock.ask(effect, HostOp::Version, 0, 0, std::ptr::null_mut(), 0.0);
            for name in [can_do::HOST_SENDS_EVENTS, can_do::HOST_SENDS_MIDI, can_do::HOST_SENDS_TIME] {
                let name = super::c_name(name);
                let _ = mock.ask(effect, HostOp::CanDo, 0, 0, name.as_ptr().cast::<c_void>().cast_mut(), 0.0);
            }
            let _ = mock.ask(effect, HostOp::GetSampleRate, 0, 0, std::ptr::null_mut(), 0.0);
            let _ = mock.ask(effect, HostOp::GetBlockSize, 0, 0, std::ptr::null_mut(), 0.0);
            // A plug-in with a panel asks for idle calls, where its editor's work belongs.
            let _ = mock.ask(effect, HostOp::NeedIdle, 0, 0, std::ptr::null_mut(), 0.0);
            0
        }
        Some(EffectOp::Close) => 0,
        Some(EffectOp::SetProgram) => {
            // Assumed: the program number travels in `value`.
            if let Ok(mut inner) = mock.0.lock() {
                inner.program = i32::try_from(value).unwrap_or(0);
            }
            0
        }
        Some(EffectOp::GetProgram) => {
            mock.0.lock().map(|inner| isize::try_from(inner.program).unwrap_or(0)).unwrap_or(0)
        }
        Some(EffectOp::GetProgramName) => {
            // SAFETY: the host's buffer, of the size the op-code is assumed to give.
            unsafe { super::write_text(ptr, vestige::VestigeMaxNameLen, "Init") };
            0
        }
        Some(EffectOp::GetParamName) => {
            if index == 0 {
                // SAFETY: the host's buffer, of the size the op-code is assumed to give.
                unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, "Gain") };
            }
            0
        }
        Some(EffectOp::GetParamLabel) => {
            if index == 0 {
                // SAFETY: the host's buffer, of the size the op-code is assumed to give.
                unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, "linear") };
            }
            0
        }
        Some(EffectOp::GetParamDisplay) => {
            if index == 0 {
                let gain = mock.0.lock().map(|inner| inner.gain).unwrap_or_default();
                // SAFETY: the host's buffer, of the size the op-code is assumed to give.
                unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, &format!("{gain:.3}")) };
            }
            0
        }
        Some(EffectOp::SetSampleRate) => {
            // Assumed: the rate travels in the call's `opt`.
            if let Ok(mut inner) = mock.0.lock() {
                inner.sample_rate = f64::from(opt);
            }
            0
        }
        Some(EffectOp::SetBlockSize) => {
            // Assumed: the size travels in the call's `value`.
            if let Ok(mut inner) = mock.0.lock() {
                inner.block_size = usize::try_from(value).unwrap_or(0);
            }
            0
        }
        Some(EffectOp::MainsChanged) => {
            // Assumed: non-zero resumes.
            if let Ok(mut inner) = mock.0.lock() {
                inner.resumed = value != 0;
            }
            0
        }
        Some(EffectOp::EditGetRect) => {
            // The op-code's `ptr` is a pointer slot, filled with a pointer to the mock's own
            // rect — the same contract a real plug-in keeps (observed: writing the rect into
            // the slot itself had the host dereference the rect's bytes as a pointer, and
            // took its process with it).
            if !ptr.is_null() {
                // SAFETY: the caller's pointer slot; the mock's rect is a static, outliving
                // the call.
                unsafe { *ptr.cast::<*const EditorRect>() = &MOCK_EDITOR_RECT };
            }
            1
        }
        Some(EffectOp::EditOpen) => {
            // The window's native handle travels in `ptr` (*assumed* per platform); recorded
            // so the tests can assert it arrived.
            let (open, handle) = (!ptr.is_null(), ptr as usize);
            if let Ok(mut inner) = mock.0.lock() {
                inner.editor_open = open;
                inner.editor_handle = handle;
            }
            1
        }
        Some(EffectOp::EditClose) => {
            if let Ok(mut inner) = mock.0.lock() {
                inner.editor_open = false;
            }
            0
        }
        Some(EffectOp::EditTop) => 0,
        Some(EffectOp::EditIdle) => {
            // The user turns the knob once, through the host's edit calls (VeSTige's
            // op-codes; the parameter in `index` and the value in `opt` are assumed).
            turn_the_knob(mock, effect);
            0
        }
        Some(EffectOp::ProcessEvents) => {
            // Assumed: `ptr` points at the `VstEvents` list; the count of events taken is
            // the answer.
            // SAFETY: the list the host handed the mock, read with a bound (`read_events`
            // caps what it takes).
            let events = unsafe { read_events(ptr.cast::<VstEvents>(), MAX_EVENTS) }.unwrap_or_default();
            let count = events.len();
            if let Ok(mut inner) = mock.0.lock() {
                bound(&mut inner.events);
                inner.pending.clone_from(&events);
                inner.events.push(events);
            }
            isize::try_from(count).unwrap_or(0)
        }
        Some(EffectOp::CanBeAutomated) => i32::from(index == 0) as isize,
        Some(EffectOp::GetPlugCategory) => vestige::kPlugCategEffect as isize,
        Some(EffectOp::GetEffectName) => {
            // SAFETY: the host's buffer, of the size the op-code is assumed to give.
            unsafe { super::write_text(ptr, vestige::VestigeMaxNameLen, NAME) };
            0
        }
        Some(EffectOp::GetVendorString) => {
            // SAFETY: the host's buffer, of the size the op-code is assumed to give.
            unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, "Suhina") };
            0
        }
        Some(EffectOp::GetProductString) => {
            // SAFETY: the host's buffer, of the size the op-code is assumed to give.
            unsafe { super::write_text(ptr, vestige::VestigeMaxLabelLen, NAME) };
            0
        }
        Some(EffectOp::GetVendorVersion) => 1,
        Some(EffectOp::CanDo) => {
            // Assumed: `ptr` names what is asked; the answer is 1 for yes.
            // SAFETY: the caller's C string, read with a bound.
            let asked = unsafe { super::c_str(ptr, 128) }.unwrap_or_default();
            let yes = matches!(
                asked.as_str(),
                can_do::ASK_RECEIVES_EVENTS | can_do::ASK_RECEIVES_MIDI | can_do::ASK_RECEIVES_TIME
            );
            isize::from(yes)
        }
        Some(EffectOp::Idle) => 0,
        Some(EffectOp::GetParameterProperties) => {
            if index == 0 && !ptr.is_null() {
                let mut properties = blank_properties();
                fill_field(&mut properties.label, "Gain");
                fill_field(&mut properties.shortLabel, "Gain");
                properties.flags = vestige::kVstParameterCanRamp;
                // SAFETY: the caller's buffer, the size of the struct the op-code fills.
                unsafe { *ptr.cast::<VstParameterProperties>() = properties };
            }
            0
        }
        Some(EffectOp::GetVstVersion) => 2400,
        Some(EffectOp::ShellGetNextPlugin) => 0,
        Some(EffectOp::StartProcess) => {
            if let Ok(mut inner) = mock.0.lock() {
                inner.processing = true;
            }
            0
        }
        Some(EffectOp::StopProcess) => {
            if let Ok(mut inner) = mock.0.lock() {
                inner.processing = false;
            }
            0
        }
        Some(EffectOp::BeginSetProgram) | Some(EffectOp::EndSetProgram) => 0,
        Some(EffectOp::GetChunk) => {
            // Assumed: `ptr` points at a pointer to fill, the answer is the chunk's size,
            // and the chunk is the gain's bytes. The chunk stays where it is until the mock's
            // next chunk call, so the host has to copy it at once (the probe does).
            let mut size = 0;
            if !ptr.is_null() {
                let chunk: Vec<u8> = mock.0.lock().map(|inner| inner.gain.to_le_bytes().to_vec()).unwrap_or_default();
                size = chunk.len();
                if let Ok(mut inner) = mock.0.lock() {
                    inner.chunk = chunk;
                    // SAFETY: the pointer the host gave to fill: it points at the mock's
                    // chunk, which lives until the next chunk call.
                    unsafe { *ptr.cast::<*const c_void>() = inner.chunk.as_ptr().cast::<c_void>() };
                }
            }
            isize::try_from(size).unwrap_or(0)
        }
        Some(EffectOp::SetChunk) => {
            // Assumed: `value` is the size and `ptr` the bytes.
            let size = usize::try_from(value).unwrap_or(0).min(MAX_CHUNK);
            let mut chunk = vec![0u8; size];
            if !ptr.is_null() && size >= 4 {
                for (slot, i) in chunk.iter_mut().zip(0..size) {
                    let byte = ptr.cast::<u8>().wrapping_add(i);
                    // SAFETY: one of the `size` bytes the host said it gave.
                    *slot = unsafe { *byte };
                }
            }
            let gain = match chunk.as_slice() {
                [a, b, c, d, ..] => f32::from_le_bytes([*a, *b, *c, *d]),
                _ => 0.0,
            };
            let applied = size >= 4 && gain.is_finite();
            if applied {
                // The editor shows the restored value: the gesture's three host calls with
                // it (VeSTige's op-codes; the value in `opt` is assumed).
                let _ = mock.ask(effect, HostOp::BeginEdit, 0, 0, std::ptr::null_mut(), 0.0);
                let _ = mock.ask(effect, HostOp::Automate, 0, 0, std::ptr::null_mut(), gain);
                let _ = mock.ask(effect, HostOp::EndEdit, 0, 0, std::ptr::null_mut(), 0.0);
            }
            if let Ok(mut inner) = mock.0.lock() {
                if applied {
                    inner.gain = gain;
                }
                inner.chunk = chunk;
            }
            0
        }
        None => 0,
    };
    mock.saw(Call { op: opcode, index, value, opt, answer: Some(answer) });
    answer
}

/// The mock's `setParameter`: the value travels as the float the call is given. Every call is
/// recorded; only the mock's one parameter takes the value.
unsafe extern "C" fn set_parameter(effect: *mut AEffect, index: i32, value: f32) {
    // SAFETY: the effect is the mock's.
    if let Some(mock) = unsafe { mock_of(effect) }
        && let Ok(mut inner) = mock.0.lock()
    {
        bound(&mut inner.parameters);
        inner.parameters.push((index, value));
        if index == 0 && value.is_finite() {
            inner.gain = value;
        }
    }
}

/// The mock's `getParameter`: the value back as a float, every call recorded.
unsafe extern "C" fn get_parameter(effect: *mut AEffect, index: i32) -> f32 {
    // SAFETY: the effect is the mock's.
    let Some(mock) = (unsafe { mock_of(effect) }) else { return 0.0 };
    mock.0
        .lock()
        .map(|mut inner| {
            bound(&mut inner.reads);
            inner.reads.push(index);
            inner.gain
        })
        .unwrap_or_default()
}

/// Turns the knob once, as a user would: the gesture's three host calls and the gain's new
/// value, the first idle or edit-idle only.
fn turn_the_knob(mock: &Mock, effect: *mut AEffect) {
    let first = mock.0.lock().map(|inner| !inner.automated).unwrap_or(false);
    if !first {
        return;
    }
    let _ = mock.ask(effect, HostOp::BeginEdit, 0, 0, std::ptr::null_mut(), 0.0);
    let _ = mock.ask(effect, HostOp::Automate, 0, 0, std::ptr::null_mut(), EDIT_GAIN);
    let _ = mock.ask(effect, HostOp::EndEdit, 0, 0, std::ptr::null_mut(), 0.0);
    if let Ok(mut inner) = mock.0.lock() {
        inner.gain = EDIT_GAIN;
        inner.automated = true;
    }
}

/// The mock's block: the gain over every channel, the notes it was given added at their
/// frames (as `test:vst3` does, so an events path is audible through either format), and the
/// host asked what a plug-in asks while it runs. `inputs` and `outputs` are the call's
/// channel pointers; a null channel is silence, and a null input buffer the same.
///
/// # Safety
///
/// The effect must be the mock's, and the channel pointers, when not null, must hold the
/// block's frames.
unsafe fn run_block(effect: *mut AEffect, inputs: *mut *mut f32, outputs: *mut *mut f32, frames: i32) {
    // SAFETY: the effect is the mock's.
    let Some(mock) = (unsafe { mock_of(effect) }) else { return };
    let (gain, first) = mock.0.lock().map(|inner| (inner.gain, inner.blocks.is_empty())).unwrap_or((0.5, true));
    let frames = usize::try_from(frames).unwrap_or(0);
    // A plug-in asks its host while it runs: the level it runs at (assumed values), and once,
    // the time information (the answer is assumed to be a pointer to it).
    let _ = mock.ask(effect, HostOp::GetCurrentProcessLevel, 0, 0, std::ptr::null_mut(), 0.0);
    if first {
        let answer = mock.ask(effect, HostOp::GetTime, 0, 0, std::ptr::null_mut(), 0.0);
        if let Some(answer) = answer.filter(|answer| *answer != 0) {
            // SAFETY: one struct read by value, at the pointer the host answered with, which
            // this layer assumes points at its live `VstTimeInfo`.
            let time = unsafe { *(answer as *const vestige::VstTimeInfo) };
            if let Ok(mut inner) = mock.0.lock() {
                inner.time_sample_rate = Some(time.sampleRate);
            }
        }
    }
    let events = mock.0.lock().map(|inner| inner.events.iter().map(Vec::len).sum()).unwrap_or(0);
    for channel in 0..CHANNELS {
        if outputs.is_null() {
            break;
        }
        let target_pointer = outputs.wrapping_add(channel);
        // SAFETY: the mock declared two outputs; this is the channel's pointer.
        let target_pointer = unsafe { *target_pointer };
        if target_pointer.is_null() {
            continue;
        }
        // SAFETY: the target channel holds the block's frames, as the call says.
        let target = unsafe { std::slice::from_raw_parts_mut(target_pointer, frames) };
        let mut source = None;
        if !inputs.is_null() {
            let input_pointer = inputs.wrapping_add(channel);
            // SAFETY: the mock declared two inputs; this is the channel's pointer.
            let input_pointer = unsafe { *input_pointer };
            if !input_pointer.is_null() {
                // SAFETY: the source channel holds the block's frames, as the call says.
                source = Some(unsafe { std::slice::from_raw_parts(input_pointer, frames) });
            }
        }
        match source {
            Some(source) => {
                for (out, sample) in target.iter_mut().zip(source) {
                    *out = *sample * gain;
                }
            }
            None => target.fill(0.0),
        }
    }
    // The notes this block's event list brought, added at their frames as `test:vst3` does,
    // so an events path is audible through either format's tests; taken, so they play once.
    let notes: Vec<MidiEvent> =
        mock.0.lock().ok().map(|mut inner| std::mem::take(&mut inner.pending)).unwrap_or_default();
    if !outputs.is_null() {
        let target_pointer = outputs.wrapping_add(0);
        // SAFETY: the mock declared outputs; the first channel's pointer.
        let first = unsafe { *target_pointer };
        if !first.is_null() {
            // SAFETY: the first channel holds the block's frames, as the call says.
            let target = unsafe { std::slice::from_raw_parts_mut(first, frames) };
            for note in notes {
                let (on, velocity) = (note.data[0] & 0xf0 == 0x90, f32::from(note.data[2]) / 127.0);
                if on && let Some(at) = usize::try_from(note.delta_samples).ok().and_then(|at| target.get_mut(at)) {
                    *at += velocity;
                }
            }
        }
    }
    if let Ok(mut inner) = mock.0.lock() {
        bound(&mut inner.blocks);
        inner.blocks.push((frames, events));
    }
}

/// The mock's deprecated `process`, kept because VeSTige names it and a host may call it.
unsafe extern "C" fn process(effect: *mut AEffect, inputs: *mut *mut f32, outputs: *mut *mut f32, frames: i32) {
    // SAFETY: the arguments the host gave the call.
    unsafe { run_block(effect, inputs, outputs, frames) }
}

/// The mock's `processReplacing`.
unsafe extern "C" fn process_replacing(
    effect: *mut AEffect,
    inputs: *mut *mut f32,
    outputs: *mut *mut f32,
    frames: i32,
) {
    // SAFETY: the arguments the host gave the call.
    unsafe { run_block(effect, inputs, outputs, frames) }
}

/// A zeroed parameter-properties struct, the base of the mock's answer.
fn blank_properties() -> VstParameterProperties {
    VstParameterProperties {
        stepFloat: 0.0,
        smallStepFloat: 0.0,
        largeStepFloat: 0.0,
        label: [0; 64],
        flags: 0,
        minInteger: 0,
        maxInteger: 0,
        stepInteger: 0,
        largeStepInteger: 0,
        shortLabel: [0; vestige::VestigeMaxShortLabelLen],
        displayIndex: 0,
        category: 0,
        numParametersInCategory: 0,
        reserved: 0,
        categoryLabel: [0; vestige::VestigeMaxCategLabelLen],
        future: [0; 16],
    }
}
