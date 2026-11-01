// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The VST2 interface layer, behind the `vst2` feature (ADR 0022): the VeSTige definitions
//! ported into [`vestige`], this module's own names for them, the mock plug-in
//! ([`test_vst2`]) and the probe ([`probe`]).
//!
//! Where every fact comes from is the point of this layer (ADR 0022 decision 4):
//!
//! - ***from VeSTige*** — a value or layout the pinned header states. It is ported into
//!   [`vestige`] and nothing here restates it.
//! - ***observed*** — behaviour our mock and probe have run, or an ignored test has seen a
//!   real installed plug-in do (`SUHINA_TEST_VST2`, `vst2_tests`).
//! - ***assumed*** — a fact the pinned header lacks, which we had to choose to write code at
//!   all: an op-code number the header does not list, which argument of a call carries what,
//!   what a return value means. It is marked at the definition and in
//!   [`docs/spec/plugin-vst2.md`](../../../docs/spec/plugin-vst2.md), and only a probe run
//!   or an ignored test against a real plug-in turns it into *observed*. A mock written from
//!   the same assumption cannot: it would agree with us by construction.
//!
//! Nothing here loads a project's plug-in yet: the host process gains the interface, the mock
//! and `--vst2-probe`. The host through the engine's boundary (step 4 of #1066) comes after,
//! on the same interface.

pub(crate) mod probe;
pub(crate) mod test_vst2;
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unsafe_op_in_unsafe_fn,
    clippy::all,
    clippy::pedantic
)]
mod vestige;

use std::ffi::c_char;
use std::ffi::c_void;

use vestige::AEffect;

/// An op-code the host sends a plug-in through `AEffect::dispatcher`. The variants' values are
/// VeSTige's where it lists the op-code; the four the pinned header lacks are marked *assumed*
/// here, in the spec, and in the probe's report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectOp {
    /// From VeSTige.
    Open,
    /// From VeSTige.
    Close,
    /// From VeSTige.
    SetProgram,
    /// From VeSTige.
    GetProgram,
    /// From VeSTige.
    GetProgramName,
    /// From VeSTige.
    GetParamName,
    /// From VeSTige. *Assumed:* the rate travels in the call's `opt` float.
    SetSampleRate,
    /// From VeSTige. *Assumed:* the size travels in the call's `value`.
    SetBlockSize,
    /// From VeSTige. *Assumed:* a non-zero `value` resumes the plug-in, zero suspends it.
    MainsChanged,
    /// From VeSTige.
    EditGetRect,
    /// From VeSTige.
    EditOpen,
    /// From VeSTige.
    EditClose,
    /// From VeSTige.
    EditIdle,
    /// From VeSTige.
    EditTop,
    /// From VeSTige. *Assumed:* `ptr` points at the `VstEvents` list.
    ProcessEvents,
    /// From VeSTige.
    CanBeAutomated,
    /// From VeSTige.
    GetPlugCategory,
    /// From VeSTige.
    GetEffectName,
    /// From VeSTige.
    GetVendorString,
    /// From VeSTige.
    GetProductString,
    /// From VeSTige.
    GetVendorVersion,
    /// From VeSTige. *Assumed:* `ptr` points at a C string naming what is asked.
    CanDo,
    /// From VeSTige.
    Idle,
    /// From VeSTige.
    GetParameterProperties,
    /// From VeSTige.
    GetVstVersion,
    /// From VeSTige.
    ShellGetNextPlugin,
    /// From VeSTige.
    StartProcess,
    /// From VeSTige.
    StopProcess,
    /// From VeSTige.
    BeginSetProgram,
    /// From VeSTige.
    EndSetProgram,
    /// *Assumed* (23): the pinned header lacks it. `index` picks the bank (0) or the current
    /// program (1), `ptr` points at a pointer the plug-in fills with its chunk, and the answer
    /// is the chunk's size in bytes.
    GetChunk,
    /// *Assumed* (24): the pinned header lacks it. `index` picks as for [`Self::GetChunk`],
    /// `value` is the chunk's size in bytes and `ptr` points at the bytes.
    SetChunk,
    /// *Assumed* (6): the pinned header lacks it. `index` is the parameter, `ptr` a buffer the
    /// plug-in fills with the parameter's unit (as [`Self::GetParamName`] fills its name).
    GetParamLabel,
    /// *Assumed* (7): the pinned header lacks it. `index` is the parameter, `ptr` a buffer the
    /// plug-in fills with the value's text.
    GetParamDisplay,
}

impl EffectOp {
    /// The op-code as it travels in the dispatcher call.
    pub(crate) fn value(self) -> i32 {
        use vestige::*;
        match self {
            Self::Open => effOpen,
            Self::Close => effClose,
            Self::SetProgram => effSetProgram,
            Self::GetProgram => effGetProgram,
            Self::GetProgramName => effGetProgramName,
            Self::GetParamName => effGetParamName,
            Self::SetSampleRate => effSetSampleRate,
            Self::SetBlockSize => effSetBlockSize,
            Self::MainsChanged => effMainsChanged,
            Self::EditGetRect => effEditGetRect,
            Self::EditOpen => effEditOpen,
            Self::EditClose => effEditClose,
            Self::EditIdle => effEditIdle,
            Self::EditTop => effEditTop,
            Self::ProcessEvents => effProcessEvents,
            Self::CanBeAutomated => effCanBeAutomated,
            Self::GetPlugCategory => effGetPlugCategory,
            Self::GetEffectName => effGetEffectName,
            Self::GetVendorString => effGetVendorString,
            Self::GetProductString => effGetProductString,
            Self::GetVendorVersion => effGetVendorVersion,
            Self::CanDo => effCanDo,
            Self::Idle => effIdle,
            Self::GetParameterProperties => effGetParameterProperties,
            Self::GetVstVersion => effGetVstVersion,
            Self::ShellGetNextPlugin => effShellGetNextPlugin,
            Self::StartProcess => effStartProcess,
            Self::StopProcess => effStopProcess,
            Self::BeginSetProgram => effBeginSetProgram,
            Self::EndSetProgram => effEndSetProgram,
            // The four the pinned header lacks: the numbers are this layer's assumption
            // (ADR 0022 decision 4), recorded in the spec and settled by the probe against
            // installed plug-ins.
            Self::GetChunk => 23,
            Self::SetChunk => 24,
            Self::GetParamLabel => 6,
            Self::GetParamDisplay => 7,
        }
    }

    /// The op-code a recorded call carried, when it is one this layer names.
    pub(crate) fn from_value(op: i32) -> Option<Self> {
        use EffectOp::*;
        Some(match op {
            0 => Open,
            1 => Close,
            2 => SetProgram,
            3 => GetProgram,
            5 => GetProgramName,
            6 => GetParamLabel,
            7 => GetParamDisplay,
            8 => GetParamName,
            10 => SetSampleRate,
            11 => SetBlockSize,
            12 => MainsChanged,
            13 => EditGetRect,
            14 => EditOpen,
            15 => EditClose,
            19 => EditIdle,
            20 => EditTop,
            23 => GetChunk,
            24 => SetChunk,
            25 => ProcessEvents,
            26 => CanBeAutomated,
            35 => GetPlugCategory,
            45 => GetEffectName,
            47 => GetVendorString,
            48 => GetProductString,
            49 => GetVendorVersion,
            51 => CanDo,
            53 => Idle,
            56 => GetParameterProperties,
            58 => GetVstVersion,
            67 => BeginSetProgram,
            68 => EndSetProgram,
            70 => ShellGetNextPlugin,
            71 => StartProcess,
            72 => StopProcess,
            _ => return None,
        })
    }

    /// The op-code the way a report names it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Open => "effOpen",
            Self::Close => "effClose",
            Self::SetProgram => "effSetProgram",
            Self::GetProgram => "effGetProgram",
            Self::GetProgramName => "effGetProgramName",
            Self::GetParamName => "effGetParamName",
            Self::SetSampleRate => "effSetSampleRate",
            Self::SetBlockSize => "effSetBlockSize",
            Self::MainsChanged => "effMainsChanged",
            Self::EditGetRect => "effEditGetRect",
            Self::EditOpen => "effEditOpen",
            Self::EditClose => "effEditClose",
            Self::EditIdle => "effEditIdle",
            Self::EditTop => "effEditTop",
            Self::ProcessEvents => "effProcessEvents",
            Self::CanBeAutomated => "effCanBeAutomated",
            Self::GetPlugCategory => "effGetPlugCategory",
            Self::GetEffectName => "effGetEffectName",
            Self::GetVendorString => "effGetVendorString",
            Self::GetProductString => "effGetProductString",
            Self::GetVendorVersion => "effGetVendorVersion",
            Self::CanDo => "effCanDo",
            Self::Idle => "effIdle",
            Self::GetParameterProperties => "effGetParameterProperties",
            Self::GetVstVersion => "effGetVstVersion",
            Self::ShellGetNextPlugin => "effShellGetNextPlugin",
            Self::StartProcess => "effStartProcess",
            Self::StopProcess => "effStopProcess",
            Self::BeginSetProgram => "effBeginSetProgram",
            Self::EndSetProgram => "effEndSetProgram",
            Self::GetChunk => "effGetChunk (assumed 23)",
            Self::SetChunk => "effSetChunk (assumed 24)",
            Self::GetParamLabel => "effGetParamLabel (assumed 6)",
            Self::GetParamDisplay => "effGetParamDisplay (assumed 7)",
        }
    }
}

/// An op-code a plug-in sends its host through the audio-master callback. Every value is from
/// VeSTige, so the whole set is named; the host answers the ones it can, and the rest are
/// recorded as they come. Which argument carries what, and what the answer means, is *assumed*
/// per op-code at the probe's answers ([`probe`]) and in the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // the set is described completely; the probe answers what a plug-in asks
pub(crate) enum HostOp {
    Automate,
    Version,
    CurrentId,
    Idle,
    PinConnected,
    WantMidi,
    GetTime,
    ProcessEvents,
    SetTime,
    TempoAt,
    GetNumAutomatableParameters,
    GetParameterQuantization,
    IOChanged,
    NeedIdle,
    SizeWindow,
    GetSampleRate,
    GetBlockSize,
    GetInputLatency,
    GetOutputLatency,
    GetPreviousPlug,
    GetNextPlug,
    WillReplaceOrAccumulate,
    GetCurrentProcessLevel,
    GetAutomationState,
    OfflineStart,
    OfflineRead,
    OfflineWrite,
    OfflineGetCurrentPass,
    OfflineGetCurrentMetaPass,
    SetOutputSampleRate,
    GetSpeakerArrangement,
    GetVendorString,
    GetProductString,
    GetVendorVersion,
    VendorSpecific,
    SetIcon,
    CanDo,
    GetLanguage,
    OpenWindow,
    CloseWindow,
    GetDirectory,
    UpdateDisplay,
    BeginEdit,
    EndEdit,
    OpenFileSelector,
    CloseFileSelector,
    EditFile,
    GetChunkFile,
    GetInputSpeakerArrangement,
}

impl HostOp {
    /// The op-code as it travels in the callback call.
    pub(crate) fn value(self) -> i32 {
        use vestige::*;
        match self {
            Self::Automate => audioMasterAutomate,
            Self::Version => audioMasterVersion,
            Self::CurrentId => audioMasterCurrentId,
            Self::Idle => audioMasterIdle,
            Self::PinConnected => audioMasterPinConnected,
            Self::WantMidi => audioMasterWantMidi,
            Self::GetTime => audioMasterGetTime,
            Self::ProcessEvents => audioMasterProcessEvents,
            Self::SetTime => audioMasterSetTime,
            Self::TempoAt => audioMasterTempoAt,
            Self::GetNumAutomatableParameters => audioMasterGetNumAutomatableParameters,
            Self::GetParameterQuantization => audioMasterGetParameterQuantization,
            Self::IOChanged => audioMasterIOChanged,
            Self::NeedIdle => audioMasterNeedIdle,
            Self::SizeWindow => audioMasterSizeWindow,
            Self::GetSampleRate => audioMasterGetSampleRate,
            Self::GetBlockSize => audioMasterGetBlockSize,
            Self::GetInputLatency => audioMasterGetInputLatency,
            Self::GetOutputLatency => audioMasterGetOutputLatency,
            Self::GetPreviousPlug => audioMasterGetPreviousPlug,
            Self::GetNextPlug => audioMasterGetNextPlug,
            Self::WillReplaceOrAccumulate => audioMasterWillReplaceOrAccumulate,
            Self::GetCurrentProcessLevel => audioMasterGetCurrentProcessLevel,
            Self::GetAutomationState => audioMasterGetAutomationState,
            Self::OfflineStart => audioMasterOfflineStart,
            Self::OfflineRead => audioMasterOfflineRead,
            Self::OfflineWrite => audioMasterOfflineWrite,
            Self::OfflineGetCurrentPass => audioMasterOfflineGetCurrentPass,
            Self::OfflineGetCurrentMetaPass => audioMasterOfflineGetCurrentMetaPass,
            Self::SetOutputSampleRate => audioMasterSetOutputSampleRate,
            Self::GetSpeakerArrangement => audioMasterGetSpeakerArrangement,
            Self::GetVendorString => audioMasterGetVendorString,
            Self::GetProductString => audioMasterGetProductString,
            Self::GetVendorVersion => audioMasterGetVendorVersion,
            Self::VendorSpecific => audioMasterVendorSpecific,
            Self::SetIcon => audioMasterSetIcon,
            Self::CanDo => audioMasterCanDo,
            Self::GetLanguage => audioMasterGetLanguage,
            Self::OpenWindow => audioMasterOpenWindow,
            Self::CloseWindow => audioMasterCloseWindow,
            Self::GetDirectory => audioMasterGetDirectory,
            Self::UpdateDisplay => audioMasterUpdateDisplay,
            Self::BeginEdit => audioMasterBeginEdit,
            Self::EndEdit => audioMasterEndEdit,
            Self::OpenFileSelector => audioMasterOpenFileSelector,
            Self::CloseFileSelector => audioMasterCloseFileSelector,
            Self::EditFile => audioMasterEditFile,
            Self::GetChunkFile => audioMasterGetChunkFile,
            Self::GetInputSpeakerArrangement => audioMasterGetInputSpeakerArrangement,
        }
    }

    /// The op-code a recorded call carried, when it is one VeSTige names.
    pub(crate) fn from_value(op: i32) -> Option<Self> {
        use HostOp::*;
        Some(match op {
            0 => Automate,
            1 => Version,
            2 => CurrentId,
            3 => Idle,
            4 => PinConnected,
            6 => WantMidi,
            7 => GetTime,
            8 => ProcessEvents,
            9 => SetTime,
            10 => TempoAt,
            11 => GetNumAutomatableParameters,
            12 => GetParameterQuantization,
            13 => IOChanged,
            14 => NeedIdle,
            15 => SizeWindow,
            16 => GetSampleRate,
            17 => GetBlockSize,
            18 => GetInputLatency,
            19 => GetOutputLatency,
            20 => GetPreviousPlug,
            21 => GetNextPlug,
            22 => WillReplaceOrAccumulate,
            23 => GetCurrentProcessLevel,
            24 => GetAutomationState,
            25 => OfflineStart,
            26 => OfflineRead,
            27 => OfflineWrite,
            28 => OfflineGetCurrentPass,
            29 => OfflineGetCurrentMetaPass,
            30 => SetOutputSampleRate,
            31 => GetSpeakerArrangement,
            32 => GetVendorString,
            33 => GetProductString,
            34 => GetVendorVersion,
            35 => VendorSpecific,
            36 => SetIcon,
            37 => CanDo,
            38 => GetLanguage,
            39 => OpenWindow,
            40 => CloseWindow,
            41 => GetDirectory,
            42 => UpdateDisplay,
            43 => BeginEdit,
            44 => EndEdit,
            45 => OpenFileSelector,
            46 => CloseFileSelector,
            47 => EditFile,
            48 => GetChunkFile,
            49 => GetInputSpeakerArrangement,
            _ => return None,
        })
    }

    /// The op-code the way a report names it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Automate => "audioMasterAutomate",
            Self::Version => "audioMasterVersion",
            Self::CurrentId => "audioMasterCurrentId",
            Self::Idle => "audioMasterIdle",
            Self::PinConnected => "audioMasterPinConnected",
            Self::WantMidi => "audioMasterWantMidi",
            Self::GetTime => "audioMasterGetTime",
            Self::ProcessEvents => "audioMasterProcessEvents",
            Self::SetTime => "audioMasterSetTime",
            Self::TempoAt => "audioMasterTempoAt",
            Self::GetNumAutomatableParameters => "audioMasterGetNumAutomatableParameters",
            Self::GetParameterQuantization => "audioMasterGetParameterQuantization",
            Self::IOChanged => "audioMasterIOChanged",
            Self::NeedIdle => "audioMasterNeedIdle",
            Self::SizeWindow => "audioMasterSizeWindow",
            Self::GetSampleRate => "audioMasterGetSampleRate",
            Self::GetBlockSize => "audioMasterGetBlockSize",
            Self::GetInputLatency => "audioMasterGetInputLatency",
            Self::GetOutputLatency => "audioMasterGetOutputLatency",
            Self::GetPreviousPlug => "audioMasterGetPreviousPlug",
            Self::GetNextPlug => "audioMasterGetNextPlug",
            Self::WillReplaceOrAccumulate => "audioMasterWillReplaceOrAccumulate",
            Self::GetCurrentProcessLevel => "audioMasterGetCurrentProcessLevel",
            Self::GetAutomationState => "audioMasterGetAutomationState",
            Self::OfflineStart => "audioMasterOfflineStart",
            Self::OfflineRead => "audioMasterOfflineRead",
            Self::OfflineWrite => "audioMasterOfflineWrite",
            Self::OfflineGetCurrentPass => "audioMasterOfflineGetCurrentPass",
            Self::OfflineGetCurrentMetaPass => "audioMasterOfflineGetCurrentMetaPass",
            Self::SetOutputSampleRate => "audioMasterSetOutputSampleRate",
            Self::GetSpeakerArrangement => "audioMasterGetSpeakerArrangement",
            Self::GetVendorString => "audioMasterGetVendorString",
            Self::GetProductString => "audioMasterGetProductString",
            Self::GetVendorVersion => "audioMasterGetVendorVersion",
            Self::VendorSpecific => "audioMasterVendorSpecific",
            Self::SetIcon => "audioMasterSetIcon",
            Self::CanDo => "audioMasterCanDo",
            Self::GetLanguage => "audioMasterGetLanguage",
            Self::OpenWindow => "audioMasterOpenWindow",
            Self::CloseWindow => "audioMasterCloseWindow",
            Self::GetDirectory => "audioMasterGetDirectory",
            Self::UpdateDisplay => "audioMasterUpdateDisplay",
            Self::BeginEdit => "audioMasterBeginEdit",
            Self::EndEdit => "audioMasterEndEdit",
            Self::OpenFileSelector => "audioMasterOpenFileSelector",
            Self::CloseFileSelector => "audioMasterCloseFileSelector",
            Self::EditFile => "audioMasterEditFile",
            Self::GetChunkFile => "audioMasterGetChunkFile",
            Self::GetInputSpeakerArrangement => "audioMasterGetInputSpeakerArrangement",
        }
    }
}

/// The `AEffect::flags` bits this layer reads.
pub(crate) mod effect_flags {
    use super::vestige;

    /// The plug-in has an editor (from VeSTige).
    pub(crate) const HAS_EDITOR: i32 = vestige::effFlagsHasEditor;
    /// The plug-in implements `processReplacing` (from VeSTige, which marks the bit "very
    /// likely").
    pub(crate) const CAN_REPLACING: i32 = vestige::effFlagsCanReplacing;
    /// The plug-in is an instrument (from VeSTige).
    pub(crate) const IS_SYNTH: i32 = vestige::effFlagsIsSynth;
    /// *Assumed* (the pinned header names no such bit): a plug-in with it saves its whole
    /// state as a chunk, through the op-codes `EffectOp::GetChunk` and `EffectOp::SetChunk`.
    pub(crate) const HAS_CHUNKS: i32 = 1 << 5;
}

/// What `audioMasterVersion` is answered with. *Assumed:* the pinned header has the op-code
/// but not the answer; 2400 stands for a host built on the 2.4 interface.
pub(crate) const HOST_VERSION: isize = 2400;

/// The answers to `audioMasterGetCurrentProcessLevel`. *Assumed:* the pinned header has the
/// op-code but not the values; the whole set is named, so the spec can list every one.
#[allow(dead_code)] // the set is complete; the host answers with the ones it uses
pub(crate) mod process_level {
    pub(crate) const UNKNOWN: isize = 0;
    pub(crate) const USER: isize = 1;
    pub(crate) const REALTIME: isize = 2;
    pub(crate) const PREFETCH: isize = 3;
    pub(crate) const OFFLINE: isize = 4;
}

/// The answers to `audioMasterGetAutomationState`. *Assumed:* the pinned header has the
/// op-code but not the values; the whole set is named, so the spec can list every one.
#[allow(dead_code)] // the set is complete; the host answers with the ones it uses
pub(crate) mod automation_state {
    pub(crate) const UNSUPPORTED: isize = 0;
    pub(crate) const OFF: isize = 1;
    pub(crate) const READ: isize = 2;
    pub(crate) const WRITE: isize = 3;
    pub(crate) const READ_WRITE: isize = 4;
}

/// The strings a `CanDo` call is answered by and asked with, as this layer spells them.
/// *Assumed:* the pinned header has both op-codes (`effCanDo`, `audioMasterCanDo`) but no
/// strings; each spelling, and who asks whom for it, is part of the assumption. A probe
/// report of what an installed plug-in asks is the observation that settles it.
pub(crate) mod can_do {
    /// The plug-in asks the host whether it sends event lists.
    pub(crate) const HOST_SENDS_EVENTS: &str = "sendVstEvents";
    /// The plug-in asks the host whether it sends MIDI events.
    pub(crate) const HOST_SENDS_MIDI: &str = "sendVstMidiEvent";
    /// The plug-in asks the host whether it answers `audioMasterGetTime`.
    pub(crate) const HOST_SENDS_TIME: &str = "sendVstTimeInfo";
    /// The host asks the plug-in whether it reads event lists.
    pub(crate) const ASK_RECEIVES_EVENTS: &str = "receiveVstEvents";
    /// The host asks the plug-in whether it reads MIDI events.
    pub(crate) const ASK_RECEIVES_MIDI: &str = "receiveVstMidiEvent";
    /// The host asks the plug-in whether it reads time information.
    pub(crate) const ASK_RECEIVES_TIME: &str = "receiveVstTimeInfo";
}

/// A `CanDo` name as the call carries it: a C string, terminated. The names are this layer's
/// own constants, which hold no zero byte, so the terminator always fits; a name that somehow
/// did not becomes the empty string, which no side says yes to.
pub(crate) fn c_name(name: &str) -> std::ffi::CString {
    std::ffi::CString::new(name).unwrap_or_default()
}

/// One call between host and plug-in, as the mock and the probe record them: the op-code as
/// it travelled and the other arguments, with the answer when the recorder is the one that
/// answered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Call {
    pub(crate) op: i32,
    pub(crate) index: i32,
    pub(crate) value: isize,
    pub(crate) opt: f32,
    /// What the call was answered with, when the recorder answered it.
    pub(crate) answer: Option<isize>,
}

/// Text from a fixed-size C string field, up to its first zero.
pub(crate) fn field_text(field: &[c_char]) -> String {
    let bytes: Vec<u8> = field.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Puts `text` into a fixed-size C string field, cut to the field's room and always
/// zero-terminated.
pub(crate) fn fill_field(field: &mut [c_char], text: &str) {
    field.fill(0);
    let room = field.len().saturating_sub(1);
    for (slot, byte) in field.iter_mut().take(room).zip(text.bytes()) {
        *slot = byte as c_char;
    }
}

/// Writes `text` into the buffer `ptr` as a zero-terminated string, zero-filling the rest of
/// the `cap` bytes the op-code's buffer is assumed to have. Nothing is written when `ptr` is
/// null.
///
/// # Safety
///
/// `ptr` must be null or point at `cap` writable bytes.
pub(crate) unsafe fn write_text(ptr: *mut c_void, cap: usize, text: &str) {
    if ptr.is_null() {
        return;
    }
    let bytes = text.as_bytes();
    let room = cap.saturating_sub(1);
    for i in 0..cap {
        let byte = if i < room { bytes.get(i).copied().unwrap_or(0) } else { 0 };
        let slot = ptr.cast::<u8>().wrapping_add(i);
        // SAFETY: one of the `cap` bytes the caller's buffer has; exactly `cap` are written,
        // so the string always ends in a zero.
        unsafe { *slot = byte };
    }
}

/// The `ptr` of a call as a C string, at most `cap` bytes, stopping at the first zero: a
/// plug-in's strings are not trusted to be shorter or terminated. `None` when `ptr` is null.
///
/// # Safety
///
/// `ptr` must point at a zero-terminated string or at `cap` readable bytes.
pub(crate) unsafe fn c_str(ptr: *const c_void, cap: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut bytes = Vec::with_capacity(cap.min(64));
    for i in 0..cap {
        let byte = ptr.cast::<u8>().wrapping_add(i);
        // SAFETY: one byte of the `cap` the caller allows; the loop stops at the first zero,
        // so a terminated string is never read past its end.
        let byte = unsafe { *byte };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// One MIDI event of a `VstEvents` list, as this layer reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MidiEvent {
    /// The event's own type field (1 is MIDI, VeSTige's `kVstMidiType`).
    pub(crate) kind: i32,
    /// How many samples into the block the event lands.
    pub(crate) delta_samples: i32,
    /// The MIDI bytes: the status byte and up to three data bytes.
    pub(crate) data: [u8; 4],
}

/// The events of a `VstEvents` list a call carries, read with a bound: at most `max` are
/// taken (a plug-in's count is not trusted), the list ends at its count or at the first null
/// event pointer, and an event whose type is not MIDI is kept by its type number alone.
/// `None` when `events` is null.
///
/// # Safety
///
/// `events` must point at a `VstEvents` whose count and event pointers the caller trusts the
/// sender enough to read; the caps here keep how far the read goes.
pub(crate) unsafe fn read_events(events: *const vestige::VstEvents, max: usize) -> Option<Vec<MidiEvent>> {
    if events.is_null() {
        return None;
    }
    // SAFETY: the struct the caller vouched for; only its count is read here.
    let count = unsafe { (*events).numEvents };
    let count = usize::try_from(count).unwrap_or(0).min(max);
    // SAFETY: as above; the event pointers begin where the struct's flexible array member
    // does, at its end.
    let pointers = unsafe { (*events).events.as_ptr() };
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let pointer = pointers.wrapping_add(i);
        // SAFETY: the `i`-th of the `count` pointers behind the struct.
        let event = unsafe { pointer.read() };
        if event.is_null() {
            break;
        }
        // SAFETY: one event the sender pointed at, read by value; its `dump` is the size of
        // a MIDI event whichever kind it is.
        let midi = unsafe { *event.cast::<vestige::VstMidiEvent>() };
        let data = if midi.r#type == vestige::kVstMidiType { midi.midiData.map(|b| b as u8) } else { [0; 4] };
        out.push(MidiEvent { kind: midi.r#type, delta_samples: midi.deltaSamples, data });
    }
    Some(out)
}

/// The events a host sends a plug-in, laid out as the interface has them: the `VstEvents`
/// head, then the event pointers in line behind it (where VeSTige's flexible array member
/// puts them), then the events themselves. Building one is safe; pointing it at its own
/// events is [`Self::ready`], called once the list is where it will stay.
#[repr(C)]
pub(crate) struct EventList<const N: usize> {
    /// The head the plug-in is handed: `numEvents` says how many of the `N` places are used.
    pub(crate) head: vestige::VstEvents,
    pointers: [*mut vestige::VstEvent; N],
    events: [vestige::VstMidiEvent; N],
}

impl<const N: usize> EventList<N> {
    /// A list of at most `N` MIDI events, each its bytes and its sample offset in the block.
    pub(crate) fn new(events: &[([u8; 4], i32)]) -> Self {
        let mut list = Self {
            head: vestige::VstEvents {
                numEvents: i32::try_from(events.len().min(N)).unwrap_or(0),
                reserved: std::ptr::null_mut(),
                events: [],
            },
            pointers: [std::ptr::null_mut(); N],
            events: [blank_midi(); N],
        };
        for (slot, (data, delta)) in list.events.iter_mut().zip(events.iter().take(N)) {
            *slot = vestige::VstMidiEvent {
                r#type: vestige::kVstMidiType,
                byteSize: std::mem::size_of::<vestige::VstMidiEvent>() as i32,
                deltaSamples: *delta,
                flags: 0,
                noteLength: 0,
                noteOffset: 0,
                midiData: [data[0] as c_char, data[1] as c_char, data[2] as c_char, data[3] as c_char],
                detune: 0,
                noteOffVelocity: 0,
                reserved1: 0,
                reserved2: 0,
            };
        }
        list
    }

    /// Points the list's event pointers at its own events and returns the head to hand the
    /// plug-in. Call it when the list has the address it will keep for the call: moving the
    /// list afterwards leaves its pointers behind.
    pub(crate) fn ready(&mut self) -> *const vestige::VstEvents {
        let used = usize::try_from(self.head.numEvents).unwrap_or(0).min(N);
        for i in 0..used {
            self.pointers[i] = &mut self.events[i] as *mut vestige::VstMidiEvent as *mut vestige::VstEvent;
        }
        &self.head
    }
}

/// A zeroed MIDI event, the base of [`EventList`]'s unused places.
const fn blank_midi() -> vestige::VstMidiEvent {
    vestige::VstMidiEvent {
        r#type: 0,
        byteSize: 0,
        deltaSamples: 0,
        flags: 0,
        noteLength: 0,
        noteOffset: 0,
        midiData: [0; 4],
        detune: 0,
        noteOffVelocity: 0,
        reserved1: 0,
        reserved2: 0,
    }
}

/// The fields of an `AEffect` a host reads before it calls anything, as the probe reports
/// them: the layout is VeSTige's; the values are the plug-in's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EffectInfo {
    pub(crate) num_programs: i32,
    pub(crate) num_params: i32,
    pub(crate) num_inputs: i32,
    pub(crate) num_outputs: i32,
    pub(crate) flags: i32,
    pub(crate) unique_id: i32,
    pub(crate) version: i32,
    /// The float VeSTige names `unkown_float` and says holds `1.0f`: what it really holds is
    /// worth a report line.
    pub(crate) unknown_float: f32,
    /// The twelve bytes VeSTige calls zeroes: what a real plug-in holds there is worth a
    /// report line, as the pinned header names no latency field and a host needs one
    /// (`docs/spec/plugin-vst2.md` › open questions).
    pub(crate) empty3: [c_char; 12],
    pub(crate) has_dispatcher: bool,
    pub(crate) has_process: bool,
    pub(crate) has_process_replacing: bool,
}

impl EffectInfo {
    /// Reads the fields of `effect`. Nothing is called and no pointer is followed.
    ///
    /// # Safety
    ///
    /// `effect` must point at an `AEffect` the caller trusts enough to read by value.
    pub(crate) unsafe fn of(effect: *const AEffect) -> Option<Self> {
        if effect.is_null() {
            return None;
        }
        // SAFETY: one struct read by value, the caller's `AEffect`.
        let effect = unsafe { *effect };
        Some(Self {
            num_programs: effect.numPrograms,
            num_params: effect.numParams,
            num_inputs: effect.numInputs,
            num_outputs: effect.numOutputs,
            flags: effect.flags,
            unique_id: effect.uniqueID,
            version: effect.version,
            unknown_float: effect.unkown_float,
            empty3: effect.empty3,
            has_dispatcher: effect.dispatcher.is_some(),
            has_process: effect.process.is_some(),
            has_process_replacing: effect.processReplacing.is_some(),
        })
    }
}

/// The tests for this layer: the layout assertions that pin the port, the mock-and-probe
/// round trip, and the ignored tests against installed plug-ins.
#[cfg(test)]
#[path = "vst2_tests.rs"]
mod vst2_tests;
