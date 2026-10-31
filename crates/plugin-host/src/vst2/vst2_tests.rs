// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tests of the VST2 interface layer. The layout assertions pin the port against the
//! pinned header's field order and C's alignment rules for each pointer width, and the
//! op-code values against the header's own numbers; the round-trip tests run the mock and
//! the probe against each other, which proves that the two halves of our own code agree and
//! nothing more (ADR 0022 decision 4); the ignored test runs the probe against an installed
//! plug-in, which is what turns the layer's *assumed* facts into *observed* ones.

use super::probe::{BLOCK_EVENTS, BLOCK_SIZE, Probe, SAMPLE_RATE};
use super::test_vst2::{self, EDIT_GAIN};
use super::vestige::{self, AEffect, VstEvents};
use super::{EffectInfo, EffectOp, EventList, HostOp, MidiEvent, effect_flags, read_events};

/// The ported layout, field by field, as C lays the structs out on this build: the field
/// order is VeSTige's and the alignment is the platform's, so the offsets are what a real
/// plug-in's struct has. VeSTige's own offset comments describe the 32-bit layout (its
/// "Id 48-4b" is 72 decimal, this build's `uniqueID`); the 64-bit offsets follow from the
/// same field order and are *assumed* against real 64-bit plug-ins until the ignored probe
/// test has read them off one (the spec's open questions).
#[test]
fn the_ported_structs_lay_out_as_the_header_orders_their_fields() {
    // `AEffect`, the layout the whole interface stands on, per pointer width: the four call
    // fields first, then the counts, then the pointers, then the tail. On 64-bit the `magic`
    // field is followed by four bytes of alignment padding; on 32-bit it is not.
    #[cfg(target_pointer_width = "64")]
    let (a_effect, tail) = (
        [
            (std::mem::offset_of!(AEffect, magic), 0),
            (std::mem::offset_of!(AEffect, dispatcher), 8),
            (std::mem::offset_of!(AEffect, process), 16),
            (std::mem::offset_of!(AEffect, setParameter), 24),
            (std::mem::offset_of!(AEffect, getParameter), 32),
            (std::mem::offset_of!(AEffect, numPrograms), 40),
            (std::mem::offset_of!(AEffect, numParams), 44),
            (std::mem::offset_of!(AEffect, numInputs), 48),
            (std::mem::offset_of!(AEffect, numOutputs), 52),
            (std::mem::offset_of!(AEffect, flags), 56),
            (std::mem::offset_of!(AEffect, ptr1), 64),
            (std::mem::offset_of!(AEffect, ptr2), 72),
            (std::mem::offset_of!(AEffect, empty3), 80),
            (std::mem::offset_of!(AEffect, unkown_float), 92),
            (std::mem::offset_of!(AEffect, ptr3), 96),
            (std::mem::offset_of!(AEffect, user), 104),
            (std::mem::offset_of!(AEffect, uniqueID), 112),
            (std::mem::offset_of!(AEffect, version), 116),
            (std::mem::offset_of!(AEffect, processReplacing), 120),
        ],
        (std::mem::size_of::<AEffect>(), 128),
    );
    #[cfg(target_pointer_width = "32")]
    let (a_effect, tail) = (
        [
            (std::mem::offset_of!(AEffect, magic), 0),
            (std::mem::offset_of!(AEffect, dispatcher), 4),
            (std::mem::offset_of!(AEffect, process), 8),
            (std::mem::offset_of!(AEffect, setParameter), 12),
            (std::mem::offset_of!(AEffect, getParameter), 16),
            (std::mem::offset_of!(AEffect, numPrograms), 20),
            (std::mem::offset_of!(AEffect, numParams), 24),
            (std::mem::offset_of!(AEffect, numInputs), 28),
            (std::mem::offset_of!(AEffect, numOutputs), 32),
            (std::mem::offset_of!(AEffect, flags), 36),
            (std::mem::offset_of!(AEffect, ptr1), 40),
            (std::mem::offset_of!(AEffect, ptr2), 44),
            (std::mem::offset_of!(AEffect, empty3), 48),
            (std::mem::offset_of!(AEffect, unkown_float), 60),
            (std::mem::offset_of!(AEffect, ptr3), 64),
            (std::mem::offset_of!(AEffect, user), 68),
            (std::mem::offset_of!(AEffect, uniqueID), 72),
            (std::mem::offset_of!(AEffect, version), 76),
            (std::mem::offset_of!(AEffect, processReplacing), 80),
        ],
        (std::mem::size_of::<AEffect>(), 84),
    );
    for (offset, expected) in a_effect {
        assert_eq!(offset, expected);
    }
    assert_eq!(tail.0, tail.1);

    // `VstMidiEvent`, all fixed-width fields: the same on every build, as VeSTige's own
    // offsets say.
    assert_eq!(std::mem::size_of::<vestige::VstMidiEvent>(), 32);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, r#type), 0);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, byteSize), 4);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, deltaSamples), 8);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, flags), 12);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, noteLength), 16);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, noteOffset), 20);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, midiData), 24);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, detune), 28);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, noteOffVelocity), 29);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, reserved1), 30);
    assert_eq!(std::mem::offset_of!(vestige::VstMidiEvent, reserved2), 31);
    assert_eq!(std::mem::size_of::<vestige::VstEvent>(), 32);

    // `VstEvents`: the head a list is handed by, and the event pointers in line behind it,
    // where VeSTige's flexible array member puts them.
    assert_eq!(std::mem::offset_of!(VstEvents, numEvents), 0);
    assert_eq!(std::mem::offset_of!(VstEvents, reserved), std::mem::size_of::<*const ()>());
    assert_eq!(std::mem::offset_of!(VstEvents, events), 2 * std::mem::size_of::<*const ()>());
    assert_eq!(std::mem::size_of::<VstEvents>(), 2 * std::mem::size_of::<*const ()>());
    let mut list = EventList::<4>::new(&BLOCK_EVENTS);
    let _ = list.ready();
    assert_eq!(std::mem::offset_of!(EventList<4>, pointers), std::mem::size_of::<VstEvents>());
    assert_eq!(
        std::mem::offset_of!(EventList<4>, events),
        std::mem::size_of::<VstEvents>() + 4 * std::mem::size_of::<*const ()>()
    );

    // `VstTimeInfo` and `VstParameterProperties`: the sizes the interface's fixed structs
    // have, all fixed-width fields, the same on every build.
    assert_eq!(std::mem::size_of::<vestige::VstTimeInfo>(), 88);
    assert_eq!(std::mem::offset_of!(vestige::VstTimeInfo, samplePos), 0);
    assert_eq!(std::mem::offset_of!(vestige::VstTimeInfo, tempo), 32);
    assert_eq!(std::mem::offset_of!(vestige::VstTimeInfo, timeSigNumerator), 64);
    assert_eq!(std::mem::offset_of!(vestige::VstTimeInfo, flags), 84);
    assert_eq!(std::mem::size_of::<vestige::VstParameterProperties>(), 152);
    assert_eq!(std::mem::offset_of!(vestige::VstParameterProperties, label), 12);
    assert_eq!(std::mem::offset_of!(vestige::VstParameterProperties, flags), 76);
    assert_eq!(std::mem::offset_of!(vestige::VstParameterProperties, shortLabel), 96);
    assert_eq!(std::mem::offset_of!(vestige::VstParameterProperties, displayIndex), 104);
    assert_eq!(std::mem::offset_of!(vestige::VstParameterProperties, categoryLabel), 112);
    assert_eq!(std::mem::offset_of!(vestige::VstParameterProperties, future), 136);
}

/// The ported values are VeSTige's own numbers, and the four the header lacks are this
/// layer's assumption, spelled out in the spec: both are pinned here, so a typo in the port
/// or a silent change of an assumption cannot slip in.
#[test]
fn the_ported_op_codes_hold_vestiges_numbers_and_our_assumed_four() {
    // From VeSTige, both directions.
    for (op, value) in [
        (EffectOp::Open, 0),
        (EffectOp::Close, 1),
        (EffectOp::SetProgram, 2),
        (EffectOp::GetProgram, 3),
        (EffectOp::GetProgramName, 5),
        (EffectOp::GetParamName, 8),
        (EffectOp::SetSampleRate, 10),
        (EffectOp::SetBlockSize, 11),
        (EffectOp::MainsChanged, 12),
        (EffectOp::EditGetRect, 13),
        (EffectOp::EditOpen, 14),
        (EffectOp::EditClose, 15),
        (EffectOp::EditIdle, 19),
        (EffectOp::EditTop, 20),
        (EffectOp::ProcessEvents, 25),
        (EffectOp::CanBeAutomated, 26),
        (EffectOp::GetPlugCategory, 35),
        (EffectOp::GetEffectName, 45),
        (EffectOp::GetVendorString, 47),
        (EffectOp::GetProductString, 48),
        (EffectOp::GetVendorVersion, 49),
        (EffectOp::CanDo, 51),
        (EffectOp::Idle, 53),
        (EffectOp::GetParameterProperties, 56),
        (EffectOp::GetVstVersion, 58),
        (EffectOp::BeginSetProgram, 67),
        (EffectOp::EndSetProgram, 68),
        (EffectOp::ShellGetNextPlugin, 70),
        (EffectOp::StartProcess, 71),
        (EffectOp::StopProcess, 72),
    ] {
        assert_eq!(op.value(), value, "{op:?}");
        assert_eq!(EffectOp::from_value(value), Some(op));
    }
    for (op, value) in [
        (HostOp::Automate, 0),
        (HostOp::Version, 1),
        (HostOp::CurrentId, 2),
        (HostOp::Idle, 3),
        (HostOp::PinConnected, 4),
        (HostOp::WantMidi, 6),
        (HostOp::GetTime, 7),
        (HostOp::ProcessEvents, 8),
        (HostOp::SetTime, 9),
        (HostOp::TempoAt, 10),
        (HostOp::IOChanged, 13),
        (HostOp::NeedIdle, 14),
        (HostOp::SizeWindow, 15),
        (HostOp::GetSampleRate, 16),
        (HostOp::GetBlockSize, 17),
        (HostOp::GetInputLatency, 18),
        (HostOp::GetOutputLatency, 19),
        (HostOp::GetCurrentProcessLevel, 23),
        (HostOp::GetAutomationState, 24),
        (HostOp::GetVendorString, 32),
        (HostOp::GetProductString, 33),
        (HostOp::GetVendorVersion, 34),
        (HostOp::VendorSpecific, 35),
        (HostOp::CanDo, 37),
        (HostOp::GetLanguage, 38),
        (HostOp::UpdateDisplay, 42),
        (HostOp::BeginEdit, 43),
        (HostOp::EndEdit, 44),
    ] {
        assert_eq!(op.value(), value, "{op:?}");
        assert_eq!(HostOp::from_value(value), Some(op));
    }
    // The four the pinned header lacks: this layer's assumption, and the spec's to settle.
    for (op, value) in [
        (EffectOp::GetParamLabel, 6),
        (EffectOp::GetParamDisplay, 7),
        (EffectOp::GetChunk, 23),
        (EffectOp::SetChunk, 24),
    ] {
        assert_eq!(op.value(), value, "{op:?} (assumed)");
    }
    // The magic, the flags and the packing, as VeSTige states them.
    assert_eq!(vestige::kEffectMagic, i32::from_le_bytes(*b"PtsV"));
    assert_eq!(vestige::effFlagsHasEditor, 1);
    assert_eq!(vestige::effFlagsCanReplacing, 1 << 4);
    assert_eq!(vestige::effFlagsIsSynth, 1 << 8);
    assert_eq!(effect_flags::HAS_CHUNKS, 1 << 5);
    assert_eq!(vestige::CCONST(b'S', b'h', b'T', b'G'), i32::from_be_bytes(*b"ShTG"));
    assert_eq!(vestige::kVstMidiType, 1);
    assert_eq!(vestige::kVstLangEnglish, 1);
}

/// The mock and the probe agree on the whole conversation, both directions: every op-code
/// the host sent arrived with the arguments this layer assumed to carry its meaning, and
/// every call the mock made was answered the way the host's answers are written. That is
/// the two halves of our own code agreeing — the *assumed* facts stay assumed.
#[test]
fn the_mock_and_the_probe_agree_on_the_conversation() {
    let (probe, mock) = Probe::mock();
    let report = probe.run();

    // What the host sent, as the mock read it.
    let ops: Vec<i32> = mock.calls().iter().map(|call| call.op).collect();
    for op in [
        EffectOp::Open,
        EffectOp::SetSampleRate,
        EffectOp::SetBlockSize,
        EffectOp::GetVstVersion,
        EffectOp::GetPlugCategory,
        EffectOp::GetEffectName,
        EffectOp::GetVendorString,
        EffectOp::GetProductString,
        EffectOp::GetVendorVersion,
        EffectOp::CanDo,
        EffectOp::GetParamName,
        EffectOp::GetParamLabel,
        EffectOp::GetParamDisplay,
        EffectOp::GetChunk,
        EffectOp::SetChunk,
        EffectOp::MainsChanged,
        EffectOp::StartProcess,
        EffectOp::ProcessEvents,
        EffectOp::EditIdle,
        EffectOp::StopProcess,
        EffectOp::Close,
    ] {
        assert!(ops.contains(&op.value()), "the host never sent {}", op.name());
    }
    assert_eq!(mock.sample_rate(), SAMPLE_RATE);
    assert_eq!(mock.block_size(), BLOCK_SIZE);
    assert_eq!(mock.blocks(), vec![(BLOCK_SIZE, BLOCK_EVENTS.len())]);
    let (resumed, processing) = mock.resumed_and_processing();
    assert!(!resumed && !processing, "the cycle closed and suspended the mock");

    // The events the host sent, as the mock read them.
    let events = mock.events();
    assert_eq!(events.len(), 1, "one event list");
    assert_eq!(
        events.first(),
        Some(&vec![
            MidiEvent { kind: 1, delta_samples: 100, data: [0x90, 60, 100, 0] },
            MidiEvent { kind: 1, delta_samples: 200, data: [0x80, 60, 64, 0] },
        ])
    );

    // What the mock asked, and what it was answered.
    let asks = mock.asks();
    let answered = |op: HostOp| asks.iter().find(|call| call.op == op.value()).and_then(|call| call.answer);
    assert_eq!(answered(HostOp::Version), Some(2400));
    assert_eq!(answered(HostOp::GetSampleRate), Some(SAMPLE_RATE as isize));
    assert_eq!(answered(HostOp::GetBlockSize), Some(BLOCK_SIZE as isize));
    assert_eq!(answered(HostOp::GetCurrentProcessLevel), Some(4));
    let answers: Vec<Option<isize>> =
        asks.iter().filter(|call| call.op == HostOp::CanDo.value()).map(|call| call.answer).collect();
    assert_eq!(answers, vec![Some(1), Some(1), Some(1)], "the host said yes to all three");
    // The gestures the mock made: the restored state reported as an edit (the chunk hand-back
    // put 0.25 in), then the knob on the edit-idle. Each gesture is the three calls, the
    // value in the automate call's float.
    let automates: Vec<f32> =
        asks.iter().filter(|call| call.op == HostOp::Automate.value()).map(|call| call.opt).collect();
    assert_eq!(automates, vec![0.25, EDIT_GAIN]);
    assert!(asks.iter().any(|call| call.op == HostOp::BeginEdit.value()));
    assert!(asks.iter().any(|call| call.op == HostOp::EndEdit.value()));
    // The time information: the answer is assumed to be a pointer to it, and the mock read
    // the rate it holds out of the pointer it was answered with.
    assert_eq!(mock.time_sample_rate(), Some(SAMPLE_RATE));

    // The gain the block applied and the idle turned, as the report shows it. The host set
    // the parameter to 0.25 before asking for the chunk, so the round trip is 0.25 to 0.25.
    assert_eq!(mock.parameters(), vec![(0, 0.25)]);
    assert_eq!(mock.gain(), EDIT_GAIN);
    assert!(report.contains("effGetChunk (assumed 23)"), "the report marks its assumptions");
    assert!(report.contains("parameter 0 was Some(0.25), is Some(0.25)"));
}

/// A chunk the plug-in gave comes back to it whole: the round trip a project's saved
/// settings stand on, with a parameter move in between to make it plain.
#[test]
fn a_chunk_round_trip_keeps_the_state_through_a_parameter_move() {
    let (mut probe, mock) = Probe::mock();
    probe.send(EffectOp::Open, 0, 0, std::ptr::null_mut(), 0.0).expect("open");
    // A setting no round trip could guess.
    probe.set_parameter(0, 0.9).expect("set");
    let mut pointer: *const std::ffi::c_void = std::ptr::null();
    let size = probe
        .send(EffectOp::GetChunk, 0, 0, &mut pointer as *mut *const std::ffi::c_void as *mut std::ffi::c_void, 0.0)
        .expect("get the chunk");
    let size = usize::try_from(size).unwrap_or(0);
    assert_eq!(size, 4, "the mock's chunk is its gain");
    let mut chunk = vec![0u8; size];
    for (slot, i) in chunk.iter_mut().zip(0..size) {
        let byte = pointer.cast::<u8>().wrapping_add(i);
        // SAFETY: the mock's chunk, of the size it answered with.
        *slot = unsafe { *byte };
    }
    assert_eq!(chunk, 0.9f32.to_le_bytes());
    // A different setting, then the chunk back: the state the chunk held comes back.
    probe.set_parameter(0, 0.1).expect("move the parameter");
    probe
        .send(EffectOp::SetChunk, 0, size as isize, chunk.as_ptr().cast::<std::ffi::c_void>().cast_mut(), 0.0)
        .expect("set the chunk");
    assert_eq!(mock.gain(), 0.9);
    assert_eq!(probe.parameter(0), Ok(0.9));
}

/// A list the host built reads back as it was built, and a list whose count lies is read to
/// its bound and no further: the reader never trusts a count.
#[test]
fn event_lists_round_trip_and_a_lying_count_reads_to_its_bound() {
    let events = [([0x90, 60, 100, 0], 7), ([0x80, 60, 64, 0], 300)];
    let mut list = EventList::<8>::new(&events);
    // SAFETY: the list is pointed at its own events, which stay where they are for the read.
    let head = list.ready();
    // SAFETY: the list this test built, whose count and pointers are its own.
    let read = unsafe { read_events(head, 1024) }.unwrap_or_default();
    assert_eq!(
        read,
        vec![
            MidiEvent { kind: 1, delta_samples: 7, data: [0x90, 60, 100, 0] },
            MidiEvent { kind: 1, delta_samples: 300, data: [0x80, 60, 64, 0] },
        ]
    );
    // A count above the bound reads to the bound.
    let mut list = EventList::<8>::new(&events);
    // SAFETY: as above.
    let head = list.ready() as *mut VstEvents;
    // SAFETY: the list this test built, to change its count.
    unsafe { (*head).numEvents = 9000 };
    // SAFETY: the same list, read with a bound of 1.
    assert_eq!(unsafe { read_events(head, 1) }.unwrap_or_default().len(), 1);
    // A negative count reads nothing.
    // SAFETY: as above.
    unsafe { (*head).numEvents = -5 };
    // SAFETY: as above.
    assert!(unsafe { read_events(head, 1024) }.unwrap_or_default().is_empty());
    // A null list reads nothing.
    // SAFETY: a null pointer, which the reader refuses.
    assert!(unsafe { read_events(std::ptr::null(), 1024) }.is_none());
}

/// A plug-in that gives no calls is refused, not called: the probe's calls are made only
/// when the plug-in has the call.
#[test]
fn a_plug_in_without_calls_is_refused_not_called() {
    // A mock whose calls are gone: every call the probe would make answers Err.
    let (mut effect, mock) = test_vst2::mock(None);
    effect.dispatcher = None;
    effect.setParameter = None;
    effect.getParameter = None;
    let mut probe = Probe::over_mock(effect, mock);
    assert_eq!(probe.send(EffectOp::Open, 0, 0, std::ptr::null_mut(), 0.0), Err("effOpen: no dispatcher".to_string()));
    assert_eq!(probe.parameter(0), Err("the plug-in has no getParameter".to_string()));
    assert_eq!(probe.set_parameter(0, 0.5), Err("the plug-in has no setParameter".to_string()));
    // No effect at all reads as none.
    // SAFETY: a null pointer, which the reader refuses.
    assert_eq!(unsafe { EffectInfo::of(std::ptr::null()) }, None);
}

/// A flag that says it replaces with no replacing call takes the deprecated call for its
/// block, over the buffers zeroed first (its outputs add to what the host left there): what
/// the production host does, and what the probe reports it went by.
#[test]
fn a_block_whose_replacing_is_gone_takes_the_deprecated_call() {
    let (mut effect, mock) = test_vst2::mock(Some(super::probe::host_callback));
    // The flag stays; the replacing call is gone.
    effect.processReplacing = None;
    let probe = Probe::over_mock(effect, mock);
    let report = probe.run();
    assert!(
        report.contains("process (deprecated; the flag said replacing and it had none)"),
        "the fallback is what the report names\n{report}"
    );
    // The sound went through: the mock's gain (0.5) applied to the ramp the block carries.
    assert!(!report.contains("the plug-in has no process call"));
}
