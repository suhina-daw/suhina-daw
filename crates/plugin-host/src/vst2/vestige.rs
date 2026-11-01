// SPDX-License-Identifier: GPL-2.0-or-later

// Ported to Rust from VeSTige as Ardour distributes it, at the commit ADR 0022 pins
// (docs/decisions/0022-vst2-interface-description.md): repository Ardour/ardour, commit
// 365cc92cba0e016c30fea34aed168a0957aeeca0, file libs/ardour/ardour/vestige/vestige.h, blob
// 0470381141768a2841916af3eeb20014528e65e6 (10 335 bytes, the file's last change 2020-08-20).
// That file is the one named exception to the clean-room rule (CONTRIBUTING.md › Clean-room);
// nothing else may be taken from another header, and facts this port lacks are established only
// by our own probe or an ignored test against a real plug-in (ADR 0022 decision 4).
//
// What the port changed, and nothing else: the C comments became `//` comments; the flexible
// array member of `VstEvents` became a zero-length array, which lays out the same; the
// function-pointer field types became named `type`s held as `Option`, so a null pointer cannot
// be called by accident; `intptr_t` became `isize` and C's fixed-width types their Rust ones.
// Every definition, value and comment is VeSTige's own; the original notice follows, kept
// verbatim.
//
// vestige.h - simple header to allow VeSTige compilation and eventually work
//
// Copyright (c) 2006 Javier Serrano Polo <jasp00/at/users.sourceforge.net>
//
// This file is part of Linux MultiMedia Studio - http://lmms.sourceforge.net
//
// This program is free software; you can redistribute it and/or
// modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation; either
// version 2 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public
// License along with this program (see COPYING); if not, write to the
// Free Software Foundation, Inc., 51 Franklin Street, Fifth Floor,
// Boston, MA 02110-1301 USA.
//
// This VeSTige header is included in this package in the good-faith
// belief that it has been cleanly and legally reverse engineered
// without reference to the official VST SDK and without its
// developer(s) having agreed to the VST SDK license agreement.

use std::ffi::c_char;
use std::ffi::c_void;

/// VeSTige's `CCONST`: four characters packed into one 32-bit integer, the first character in
/// the most significant byte. C's shift overflows into the sign bit when the first character
/// is at least `0x80`; `wrapping_shl` keeps that behaviour instead of panicking.
pub const fn CCONST(a: u8, b: u8, c: u8, d: u8) -> i32 {
    ((a as i32).wrapping_shl(24)) | ((b as i32).wrapping_shl(16)) | ((c as i32).wrapping_shl(8)) | (d as i32)
}

// The op-codes a plug-in sends its host through the audio-master callback.

pub const audioMasterAutomate: i32 = 0;
pub const audioMasterVersion: i32 = 1;
pub const audioMasterCurrentId: i32 = 2;
pub const audioMasterIdle: i32 = 3;
pub const audioMasterPinConnected: i32 = 4;
// unsupported? 5
pub const audioMasterWantMidi: i32 = 6;
pub const audioMasterGetTime: i32 = 7;
pub const audioMasterProcessEvents: i32 = 8;
pub const audioMasterSetTime: i32 = 9;
pub const audioMasterTempoAt: i32 = 10;
pub const audioMasterGetNumAutomatableParameters: i32 = 11;
pub const audioMasterGetParameterQuantization: i32 = 12;
pub const audioMasterIOChanged: i32 = 13;
pub const audioMasterNeedIdle: i32 = 14;
pub const audioMasterSizeWindow: i32 = 15;
pub const audioMasterGetSampleRate: i32 = 16;
pub const audioMasterGetBlockSize: i32 = 17;
pub const audioMasterGetInputLatency: i32 = 18;
pub const audioMasterGetOutputLatency: i32 = 19;
pub const audioMasterGetPreviousPlug: i32 = 20;
pub const audioMasterGetNextPlug: i32 = 21;
pub const audioMasterWillReplaceOrAccumulate: i32 = 22;
pub const audioMasterGetCurrentProcessLevel: i32 = 23;
pub const audioMasterGetAutomationState: i32 = 24;
pub const audioMasterOfflineStart: i32 = 25;
pub const audioMasterOfflineRead: i32 = 26;
pub const audioMasterOfflineWrite: i32 = 27;
pub const audioMasterOfflineGetCurrentPass: i32 = 28;
pub const audioMasterOfflineGetCurrentMetaPass: i32 = 29;
pub const audioMasterSetOutputSampleRate: i32 = 30;
// unsupported? 31
pub const audioMasterGetSpeakerArrangement: i32 = 31; // deprecated in 2.4?
pub const audioMasterGetVendorString: i32 = 32;
pub const audioMasterGetProductString: i32 = 33;
pub const audioMasterGetVendorVersion: i32 = 34;
pub const audioMasterVendorSpecific: i32 = 35;
pub const audioMasterSetIcon: i32 = 36;
pub const audioMasterCanDo: i32 = 37;
pub const audioMasterGetLanguage: i32 = 38;
pub const audioMasterOpenWindow: i32 = 39;
pub const audioMasterCloseWindow: i32 = 40;
pub const audioMasterGetDirectory: i32 = 41;
pub const audioMasterUpdateDisplay: i32 = 42;
pub const audioMasterBeginEdit: i32 = 43; //BeginGesture
pub const audioMasterEndEdit: i32 = 44; //EndGesture
pub const audioMasterOpenFileSelector: i32 = 45;
pub const audioMasterCloseFileSelector: i32 = 46; // currently unused
pub const audioMasterEditFile: i32 = 47; // currently unused
pub const audioMasterGetChunkFile: i32 = 48; // currently unused
pub const audioMasterGetInputSpeakerArrangement: i32 = 49; // currently unused

// The flags of `AEffect::flags`. VeSTige marks `effFlagsCanReplacing` "very likely" and the
// others as it found them; a flag's exact bit is a fact of the interface, and only these three
// are in the pinned header.

pub const effFlagsHasEditor: i32 = 1;
pub const effFlagsCanReplacing: i32 = 1 << 4; // very likely
pub const effFlagsIsSynth: i32 = 1 << 8; // currently unused

// The op-codes a host sends a plug-in through `AEffect::dispatcher`.

pub const effOpen: i32 = 0;
pub const effClose: i32 = 1; // currently unused
pub const effSetProgram: i32 = 2; // currently unused
pub const effGetProgram: i32 = 3; // currently unused
pub const effGetProgramName: i32 = 5; // currently unused
pub const effGetParamName: i32 = 8; // currently unused
pub const effSetSampleRate: i32 = 10;
pub const effSetBlockSize: i32 = 11;
pub const effMainsChanged: i32 = 12;
pub const effEditGetRect: i32 = 13;
pub const effEditOpen: i32 = 14;
pub const effEditClose: i32 = 15;
pub const effEditIdle: i32 = 19;
pub const effEditTop: i32 = 20;
pub const effProcessEvents: i32 = 25;
pub const effCanBeAutomated: i32 = 26;
// the next one from http://asseca.com/vst-24-specs/index.html
pub const effGetPlugCategory: i32 = 35;
pub const effGetEffectName: i32 = 45;
pub const effGetVendorString: i32 = 47;
pub const effGetProductString: i32 = 48;
pub const effGetVendorVersion: i32 = 49;
pub const effCanDo: i32 = 51; // currently unused
/* from http://asseca.com/vst-24-specs/efIdle.html */
pub const effIdle: i32 = 53;
/* from http://asseca.com/vst-24-specs/efGetParameterProperties.html */
pub const effGetParameterProperties: i32 = 56;
pub const effGetVstVersion: i32 = 58; // currently unused
/* http://asseca.com/vst-24-specs/efShellGetNextPlugin.html */
pub const effShellGetNextPlugin: i32 = 70;
/* The next two were gleaned from http://www.kvraudio.com/forum/printview.php?t=143587&start=0 */
pub const effStartProcess: i32 = 71;
pub const effStopProcess: i32 = 72;

pub const effBeginSetProgram: i32 = 67;
pub const effEndSetProgram: i32 = 68;

/// The first field of every `AEffect`: "PtsV" on a little-endian build, "VstP" on a big-endian
/// one, as the four bytes are laid out in memory.
pub const kEffectMagic: i32 = if cfg!(target_endian = "big") {
    // "VstP"
    0x50747356
} else {
    // "PtsV"
    0x56737450
};

pub const kVstLangEnglish: i32 = 1;
pub const kVstMidiType: i32 = 1;

// The flags of `VstTimeInfo::flags`: what the transport is doing, and which of its fields hold
// a value this call.

pub const kVstTransportChanged: i32 = 1;
pub const kVstTransportPlaying: i32 = 1 << 1;
pub const kVstTransportCycleActive: i32 = 1 << 2;
pub const kVstTransportRecording: i32 = 1 << 3;

pub const kVstAutomationWriting: i32 = 1 << 6;
pub const kVstAutomationReading: i32 = 1 << 7;

pub const kVstNanosValid: i32 = 1 << 8;
pub const kVstPpqPosValid: i32 = 1 << 9;
pub const kVstTempoValid: i32 = 1 << 10;
pub const kVstBarsValid: i32 = 1 << 11;
pub const kVstCyclePosValid: i32 = 1 << 12;
pub const kVstTimeSigValid: i32 = 1 << 13;
pub const kVstSmpteValid: i32 = 1 << 14;
pub const kVstClockValid: i32 = 1 << 15;

/// One MIDI event as it travels in a [`VstEvents`] list. The offset comments are VeSTige's own
/// (the `?` ones are its uncertainty); the struct is the same size on every build, as it holds
/// no pointer.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VstMidiEvent {
    // 00
    pub r#type: i32,
    // 04
    pub byteSize: i32,
    // 08
    pub deltaSamples: i32,
    // 0c?
    pub flags: i32,
    // 10?
    pub noteLength: i32,
    // 14?
    pub noteOffset: i32,
    // 18
    pub midiData: [c_char; 4],
    // 1c?
    pub detune: c_char,
    // 1d?
    pub noteOffVelocity: c_char,
    // 1e?
    pub reserved1: c_char,
    // 1f?
    pub reserved2: c_char,
}

/// The size VeSTige gives `VstEvent::dump`, checked against the struct it is taken from.
const _: () = assert!(std::mem::size_of::<VstMidiEvent>() == 32);

/// Any event in a [`VstEvents`] list: VeSTige's `char dump[sizeof (VstMidiEvent)]`, the bytes
/// of whichever event kind the list holds, read back as that kind by the receiver.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VstEvent {
    pub dump: [c_char; 32],
}

/// A list of events a host hands a plug-in (`effProcessEvents`) or a plug-in hands its host
/// (`audioMasterProcessEvents`): the count, a reserved pointer, then the event pointers in
/// line behind the struct. VeSTige's `VstEvent * events[]` is a flexible array member, which
/// Rust has not; the port models it as a zero-length array, so the struct's size is the bytes
/// before the events and `events` names where they begin.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VstEvents {
    // 00
    pub numEvents: i32,
    // 04
    pub reserved: *mut c_void,
    // 08
    pub events: [*mut VstEvent; 0],
}

// constants from http://www.rawmaterialsoftware.com/juceforum/viewtopic.php?t=3740&sid=183f74631fee71a493316735e2b9f28b

/// The lengths of the fixed-size character fields a plug-in fills for the host.
pub const VestigeMaxNameLen: usize = 64;
pub const VestigeMaxLabelLen: usize = 128;
pub const VestigeMaxShortLabelLen: usize = 8;
pub const VestigeMaxCategLabelLen: usize = 24;
pub const VestigeMaxFileNameLen: usize = 100;

// constants from http://asseca.com/vst-24-specs/efGetPlugCategory.html

/// What kind of plug-in it is, as `effGetPlugCategory` answers.
pub type VstPlugCategory = i32;

pub const kPlugCategUnknown: VstPlugCategory = 0;
pub const kPlugCategEffect: VstPlugCategory = 1;
pub const kPlugCategSynth: VstPlugCategory = 2;
pub const kPlugCategAnalysis: VstPlugCategory = 3;
pub const kPlugCategMastering: VstPlugCategory = 4;
pub const kPlugCategSpacializer: VstPlugCategory = 5;
pub const kPlugCategRoomFx: VstPlugCategory = 6;
pub const kPlugSurroundFx: VstPlugCategory = 7;
pub const kPlugCategRestoration: VstPlugCategory = 8;
pub const kPlugCategOfflineProcess: VstPlugCategory = 9;
pub const kPlugCategShell: VstPlugCategory = 10;
pub const kPlugCategGenerator: VstPlugCategory = 11;
pub const kPlugCategMaxCount: VstPlugCategory = 12;

// this struct taken from http://asseca.com/vst-24-specs/efGetParameterProperties.html

/// What a parameter is, as `effGetParameterProperties` fills it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VstParameterProperties {
    pub stepFloat: f32,                                   /* float step */
    pub smallStepFloat: f32,                              /* small float step */
    pub largeStepFloat: f32,                              /* large float step */
    pub label: [c_char; 64],                              /* parameter label */
    pub flags: i32,                                       /* @see VstParameterFlags */
    pub minInteger: i32,                                  /* integer minimum */
    pub maxInteger: i32,                                  /* integer maximum */
    pub stepInteger: i32,                                 /* integer step */
    pub largeStepInteger: i32,                            /* large integer step */
    pub shortLabel: [c_char; VestigeMaxShortLabelLen],    /* short label, recommended: 6 + delimiter */
    pub displayIndex: i16, /* index where this parameter should be displayed (starting with 0) */
    pub category: i16,     /* 0: no category, else group index + 1 */
    pub numParametersInCategory: i16, /* number of parameters in category */
    pub reserved: i16,     /* zero */
    pub categoryLabel: [c_char; VestigeMaxCategLabelLen], /* category label, e.g. "Osc 1"  */
    pub future: [c_char; 16], /* reserved for future use */
}

// this enum taken from http://asseca.com/vst-24-specs/efGetParameterProperties.html

/// The flags of `VstParameterProperties::flags`.
pub type VstParameterFlags = i32;

pub const kVstParameterIsSwitch: VstParameterFlags = 1 << 0; /* parameter is a switch (on/off) */
pub const kVstParameterUsesIntegerMinMax: VstParameterFlags = 1 << 1; /* minInteger, maxInteger valid */
pub const kVstParameterUsesFloatStep: VstParameterFlags = 1 << 2; /* stepFloat, smallStepFloat, largeStepFloat valid */
pub const kVstParameterUsesIntStep: VstParameterFlags = 1 << 3; /* stepInteger, largeStepInteger valid */
pub const kVstParameterSupportsDisplayIndex: VstParameterFlags = 1 << 4; /* displayIndex valid */
pub const kVstParameterSupportsDisplayCategory: VstParameterFlags = 1 << 5; /* category, etc. valid */
pub const kVstParameterCanRamp: VstParameterFlags = 1 << 6; /* set if parameter value can ramp up/down */

/// The signature of `AEffect::dispatcher`, the one call a host makes for everything but audio
/// and parameter values. `value` carries a number (its meaning is the op-code's), `ptr` a
/// pointer to fill or read, `opt` a float.
pub type DispatcherFn = unsafe extern "C" fn(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize;

/// The signature of `AEffect::process` and `AEffect::processReplacing`: interleaved channels
/// behind pointers, in and out, and how many frames.
pub type ProcessFn =
    unsafe extern "C" fn(effect: *mut AEffect, inputs: *mut *mut f32, outputs: *mut *mut f32, sampleFrames: i32);

/// The signature of `AEffect::setParameter`: one parameter index and its new value.
pub type SetParameterFn = unsafe extern "C" fn(effect: *mut AEffect, index: i32, value: f32);

/// The signature of `AEffect::getParameter`: one parameter index, its value back.
pub type GetParameterFn = unsafe extern "C" fn(effect: *mut AEffect, index: i32) -> f32;

/// The handle a VST2 library's entry point returns: one per instance. VeSTige's own warning
/// stands: never use virtual functions on it — the struct is plain data the plug-in filled.
///
/// The field comments are VeSTige's, and they annotate the 32-bit layout; on a 64-bit build
/// the pointer-sized fields take eight bytes each and the offsets move (pinned per build by
/// the tests in `vst2_tests`, and *assumed* against real 64-bit plug-ins until the probe has
/// read them off one; see `docs/spec/plugin-vst2.md`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AEffect {
    // 00-03
    pub magic: i32,
    // dispatcher 04-07
    pub dispatcher: Option<DispatcherFn>,
    // process, quite sure 08-0b
    pub process: Option<ProcessFn>,
    // setParameter 0c-0f
    pub setParameter: Option<SetParameterFn>,
    // getParameter 10-13
    pub getParameter: Option<GetParameterFn>,
    // programs 14-17
    pub numPrograms: i32,
    // Params 18-1b
    pub numParams: i32,
    // Input 1c-1f
    pub numInputs: i32,
    // Output 20-23
    pub numOutputs: i32,
    // flags 24-27
    pub flags: i32,
    // Fill somewhere 28-2b
    pub ptr1: *mut c_void,
    pub ptr2: *mut c_void,
    // Zeroes 2c-2f 30-33 34-37 38-3b
    pub empty3: [c_char; 4 + 4 + 4],
    // 1.0f 3c-3f (the field name is VeSTige's own spelling)
    pub unkown_float: f32,
    // An object? pointer 40-43
    pub ptr3: *mut c_void,
    // Zeroes 44-47
    pub user: *mut c_void,
    // Id 48-4b
    pub uniqueID: i32,
    // Version 4c-4f
    pub version: i32,
    // processReplacing 50-53
    pub processReplacing: Option<ProcessFn>,
}

/// The song's position and tempo as a host reports it to `audioMasterGetTime`, and as a
/// plug-in asks to have it set (`audioMasterSetTime`). Which fields hold a value is what
/// `flags` says (the `kVst…Valid` constants above).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VstTimeInfo {
    /* info from online documentation of VST provided by Steinberg */
    pub samplePos: f64,
    pub sampleRate: f64,
    pub nanoSeconds: f64,
    pub ppqPos: f64,
    pub tempo: f64,
    pub barStartPos: f64,
    pub cycleStartPos: f64,
    pub cycleEndPos: f64,
    pub timeSigNumerator: i32,
    pub timeSigDenominator: i32,
    pub smpteOffset: i32,
    pub smpteFrameRate: i32,
    pub samplesToNextClock: i32,
    pub flags: i32,
}

/// The callback a host hands the plug-in's entry point, through which the plug-in asks the
/// host for everything but audio: the same argument shapes as the dispatcher, the other way.
pub type AudioMasterCallback = Option<
    unsafe extern "C" fn(
        effect: *mut AEffect,
        opcode: i32,
        index: i32,
        value: isize,
        ptr: *mut c_void,
        opt: f32,
    ) -> isize,
>;
