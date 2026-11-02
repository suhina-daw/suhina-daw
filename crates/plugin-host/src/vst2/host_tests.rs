// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's tests, over a real link in this process: the built-in mock through the whole
//! path a project's plug-in takes — load, blocks with events and automation, parameters,
//! state as a bank, the idle knob reported as an edit, a reconfigure. What these prove is
//! that the host and the mock agree; the *assumed* facts they both stand on are settled by
//! the probe and the ignored test against installed plug-ins (ADR 0022 decision 4).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use suhina_ipc::{Event, EventKind, Link, Outcome, Served};

use super::{Module, Vst2Plugin};
use crate::vst2::test_vst2::LATENCY;

/// How long a block waits for its answer.
const PATIENT: Duration = Duration::from_secs(10);
/// Instance numbers above the ones other tests of this process use, so their audio threads
/// never serve each other's links.
const FIRST: u32 = 20_001;

/// The mock loaded as instance `number`, with its link.
fn loaded(number: u32) -> (Link, Vst2Plugin) {
    let link = Link::create(2, 256).unwrap_or_else(|e| panic!("{e}"));
    let served = Served::open(&link.address()).unwrap_or_else(|e| panic!("{e}"));
    let plugin = Vst2Plugin::load("test:vst2", number, served, 48_000.0).unwrap_or_else(|e| panic!("{e}"));
    (link, plugin)
}

/// Stops the plug-in as the process would unload it: its audio thread, then its close.
fn finished(mut plugin: Vst2Plugin) {
    assert!(plugin.stop_audio(PATIENT), "the audio thread stops");
    plugin.close();
}

/// One block through the link, with `events` at their frames.
fn block(link: &mut Link, input: &[f32], events: &[Event]) -> Vec<f32> {
    let mut output = vec![0.0f32; input.len()];
    let frames = input.len() / 2;
    let mut all: Vec<Event> =
        vec![Event { frame: 0, note: -1, kind: EventKind::Transport { playing: true, position: 0 } }];
    all.extend(events.iter().copied());
    let _ = frames;
    assert_eq!(link.process_with_events(input, &all, &mut output, PATIENT), Outcome::Done, "the block was answered");
    output
}

/// The mock loads with the latency it reports in the field this host assumes holds it,
/// processes a block with the events it was given, and follows a parameter set by the block.
#[test]
fn the_mock_runs_blocks_events_and_automation() {
    let (mut link, plugin) = loaded(FIRST);
    // The latency, read from the first of VeSTige's "zero" bytes, agrees with the mock's.
    assert_eq!(plugin.latency(), LATENCY.max(0) as u32);

    // A plain block: the input times the mock's gain (0.5 at first).
    let input: Vec<f32> = (0..512).map(|i| (i % 24) as f32 / 24.0).collect();
    let output = block(&mut link, &input, &[]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * 0.5).abs() < 1e-6, "{out} vs {sample}");
    }

    // The same block with a note at its frame: the velocity is added there, as `test:vst3`
    // adds it, so an events path is audible through either format.
    let note = Event { frame: 10, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } };
    let output = block(&mut link, &input, &[note]);
    assert!((output[10 * 2] - input[10 * 2] * 0.5 - 1.0).abs() < 1e-6, "{}", output[20]);
    assert!((output[11 * 2] - input[11 * 2] * 0.5).abs() < 1e-6, "only the note's frame");

    // A parameter the block carries (automation) reaches the plug-in before its audio, so
    // the same block plays with it.
    let change = Event { frame: 0, note: -1, kind: EventKind::Param { id: 0, value: 0.9 } };
    let output = block(&mut link, &input, &[change]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * 0.9).abs() < 1e-6, "{out} vs {sample}");
    }
    finished(plugin);
}

/// The mock's parameters list as the plug-in's own range: one gain, 0 to 1, its value now
/// (the header names no op-code for a default).
#[test]
fn the_mock_lists_its_parameters() {
    let (_link, mut plugin) = loaded(FIRST + 1);
    let params = plugin.params().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].id, 0);
    assert_eq!(params[0].name, "Gain");
    assert_eq!((params[0].min, params[0].max), (0.0, 1.0));
    assert_eq!(params[0].value, 0.5);
    assert!(params[0].automatable);
    // A set parameter takes and reads back.
    let set = plugin.set_param(0, 0.25).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(set.value, 0.25);
    assert_eq!(plugin.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.25);
    // An id the plug-in has not is refused, not set.
    assert!(plugin.set_param(7, 0.5).is_err());
    finished(plugin);
}

/// A state the mock was given takes in the plug-in and is NOT reported as a parameter
/// change: reading a state is neither a change nor an undo step (ADR 0010 decision 7), so a
/// project that opens does not mark itself unsaved through its plug-in's own report of the
/// restored value.
#[test]
fn a_restored_state_is_not_reported_as_a_parameter_change() {
    let (_link, mut plugin) = loaded(FIRST + 2);
    let bank = suhina_plugins::vst2::bank(
        &suhina_plugins::vst2::Preset {
            kind: suhina_plugins::vst2::CHUNK_BANK,
            id: 7,
            version: 1,
            programs: 1,
            current: 0,
            parameters: 1,
            little_endian: false,
        },
        &0.25f32.to_le_bytes(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    plugin.set_state(&bank).unwrap_or_else(|e| panic!("{e}"));
    // The value took (the plug-in runs with it) and nothing came back as an edit.
    assert_eq!(plugin.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.25);
    assert_eq!(plugin.defaults.borrow().first().copied(), Some(0.5), "the default is the load-time value");
    assert!(plugin.tick().is_empty());
    finished(plugin);
}

/// A state round trip as the standard bank: the mock's chunk in a chunk bank, and a program
/// bank's current program set as parameters. A second instance stands in for a reopened
/// project.
#[test]
fn a_state_round_trips_through_the_bank_forms() {
    let (_link, mut first) = loaded(FIRST + 3);
    first.set_param(0, 0.25).unwrap_or_else(|e| panic!("{e}"));
    let state = first.state().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(&state[..4], b"CcnK", "the standard bank");
    assert_eq!(&state[8..12], b"FBCh", "the mock has a chunk");
    assert_eq!(suhina_plugins::vst2::chunk(&state), Some(0.25f32.to_le_bytes().to_vec().as_slice()));

    let (_link, mut reopened) = loaded(FIRST + 4);
    reopened.set_state(&state).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.25);

    // A program bank: its current program's values reach the plug-in as parameters.
    let preset = suhina_plugins::vst2::Preset {
        kind: suhina_plugins::vst2::PROGRAM_BANK,
        id: 7,
        version: 1,
        programs: 1,
        current: 0,
        parameters: 1,
        little_endian: false,
    };
    let mut buffer = vec![0u8; 28];
    buffer.extend_from_slice(&0.75f32.to_bits().to_be_bytes());
    let bank = suhina_plugins::vst2::bank(&preset, &buffer).unwrap_or_else(|e| panic!("{e}"));
    reopened.set_state(&bank).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.75);
    finished(reopened);
    finished(first);
}

/// A reconfigure to another sample rate stops and restarts the plug-in's audio on the same
/// link, and the blocks go on.
#[test]
fn a_reconfigure_keeps_the_plug_in_running() {
    let (mut link, mut plugin) = loaded(FIRST + 5);
    assert_eq!(plugin.reconfigure(96_000.0).unwrap_or_else(|e| panic!("{e}")), plugin.latency());
    let input: Vec<f32> = (0..512).map(|i| (i % 24) as f32 / 24.0).collect();
    let output = block(&mut link, &input, &[]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * 0.5).abs() < 1e-6, "{out} vs {sample}");
    }
    // The same rate again changes nothing and answers at once.
    assert_eq!(plugin.reconfigure(96_000.0).unwrap_or_else(|e| panic!("{e}")), plugin.latency());
    finished(plugin);
}

/// Whether a bank's bytes are cut short is the state's reader's to refuse, not the plug-in's
/// to guess at: a bank whose shape says `CcnK` and a bank kind, and that neither reader took,
/// is an error — never slipped in as a bare chunk the plug-in would take as its own bytes.
#[test]
fn a_bank_shape_neither_reader_took_is_refused() {
    let (_link, mut plugin) = loaded(FIRST + 6);
    let bank = suhina_plugins::vst2::bank(
        &suhina_plugins::vst2::Preset {
            kind: suhina_plugins::vst2::CHUNK_BANK,
            id: 7,
            version: 1,
            programs: 1,
            current: 0,
            parameters: 1,
            little_endian: false,
        },
        &0.25f32.to_le_bytes(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    // Neither reader takes it once it is cut short; its shape is a bank's just the same.
    let cut = &bank[..bank.len().saturating_sub(4)];
    assert!(suhina_plugins::vst2::chunk(cut).is_none());
    let refused = plugin.set_state(cut);
    assert!(refused.is_err(), "a cut-short bank is refused: {refused:?}");
    // Nothing was handed to the plug-in: its gain is the one it opened with.
    assert_eq!(plugin.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.5);
    finished(plugin);
}

/// The counts a plug-in declares are refused over the cap, as the block's are: a count too
/// big to lay arrays out for is refused, not clamped to a shape its own code strays past.
#[test]
fn declared_channel_counts_over_the_cap_are_refused() {
    assert!(super::channel_cap(0, 0).is_none(), "a silent plug-in's counts might be none");
    assert!(super::channel_cap(super::MAX_CHANNELS, super::MAX_CHANNELS).is_none());
    assert!(super::channel_cap(0, 130).is_none(), "an installed instrument's outputs");
    let Some(what) = super::channel_cap(super::MAX_CHANNELS + 1, 0) else {
        panic!("over the cap is refused");
    };
    assert!(what.contains("inputs"), "{what}");
    assert!(
        !what.contains("outputs") || what.contains("0 outputs"),
        "the message on the refused side names both counts: {what}"
    );
    let Some(what) = super::channel_cap(0, super::MAX_CHANNELS + 1) else {
        panic!("either side over is refused");
    };
    assert!(what.contains("outputs"), "{what}");
}

/// The arrangement's loop in seconds is no fact of a VST2's cycle fields: the beat cycle's
/// positions are what the fields hold, so a seconds-only block leaves the cycle validity off
/// and the positions as they were.
#[test]
fn the_arrangement_s_loop_in_seconds_is_no_vst2_cycle() {
    let (mut link, plugin) = loaded(FIRST + 7);
    let looped = Event { frame: 0, note: -1, kind: EventKind::CycleSeconds { start: 4.5, end: 12.25 } };
    let input: Vec<f32> = (0..512).map(|i| (i % 24) as f32 / 24.0).collect();
    let _output = block(&mut link, &input, &[looped]);
    let flags = plugin.shared.time.flags.load(Ordering::Relaxed);
    assert_eq!(flags & super::vestige::kVstCyclePosValid, 0, "the cycle stays invalid without beats");
    assert_eq!(plugin.shared.time.cycle_start.load(Ordering::Relaxed), 0, "no seconds in a beat field");
    assert_ne!(flags & super::vestige::kVstTransportPlaying, 0, "the transport itself was told");
    finished(plugin);
}

/// The arrangement's loop in beats tells a plug-in whether it is on: the positions stay valid
/// and the active flag follows.
#[test]
fn the_loop_s_state_follows_its_switch() {
    let (mut link, plugin) = loaded(FIRST + 9);
    let input: Vec<f32> = vec![0.0; 512];
    let flags = |link: &mut Link, on: bool| {
        let cycle = Event { frame: 0, note: -1, kind: EventKind::Cycle { on, start: 4.0, end: 8.0 } };
        let _output = block(link, &input, &[cycle]);
        plugin.shared.time.flags.load(Ordering::Relaxed)
    };
    let on = flags(&mut link, true);
    assert_ne!(on & super::vestige::kVstTransportCycleActive, 0, "on");
    assert_ne!(on & super::vestige::kVstCyclePosValid, 0);
    let off = flags(&mut link, false);
    assert_eq!(off & super::vestige::kVstTransportCycleActive, 0, "off");
    assert_ne!(off & super::vestige::kVstCyclePosValid, 0, "the positions are still the loop's");
    finished(plugin);
}

/// A block's parameter event reaches the plug-in only for a parameter it declared, in its range.
#[test]
fn a_block_s_parameter_stays_inside_what_the_plug_in_declared() {
    let (mut link, plugin) = loaded(FIRST + 10);
    let Module::Mock(_, mock) = &plugin.module else { panic!("the mock") };
    let input: Vec<f32> = vec![0.0; 512];
    let param = |id, value| Event { frame: 0, note: -1, kind: EventKind::Param { id, value } };
    let _output = block(&mut link, &input, &[param(5000, 0.5), param(0, 7.0)]);
    assert_eq!(mock.parameters(), vec![(0, 1.0)], "only the declared parameter, in its range");
    finished(plugin);
}

/// A plug-in that asks its host from any call of a block is answered as the audio thread it runs
/// on: from the instance this thread serves, so without the instance table's lock.
#[test]
fn every_call_of_a_block_asks_its_host_without_the_table_lock() {
    let (mut link, plugin) = loaded(FIRST + 11);
    let Module::Mock(_, mock) = &plugin.module else { panic!("the mock") };
    let input: Vec<f32> = vec![0.0; 512];
    let note = Event { frame: 0, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } };
    let change = Event { frame: 0, note: -1, kind: EventKind::Param { id: 0, value: 0.5 } };
    let _output = block(&mut link, &input, &[change, note]);
    let levels: Vec<Option<isize>> = mock
        .asks()
        .iter()
        .filter(|call| call.op == super::HostOp::GetCurrentProcessLevel.value())
        .map(|call| call.answer)
        .collect();
    // `setParameter`, `effProcessEvents` and the audio call.
    assert_eq!(levels, vec![Some(super::process_level::REALTIME); 3]);
    finished(plugin);
}

/// What an `effSetChunk` answer means is not observed, so a non-zero one fails nothing.
#[test]
fn an_answer_to_a_chunk_is_no_refusal() {
    unsafe extern "C" fn answers_one(
        _effect: *mut super::AEffect,
        _opcode: i32,
        _index: i32,
        _value: isize,
        _ptr: *mut std::ffi::c_void,
        _opt: f32,
    ) -> isize {
        1
    }
    let (_link, mut plugin) = loaded(FIRST + 12);
    let real = plugin.dispatcher.replace(answers_one);
    let bank = suhina_plugins::vst2::bank(
        &suhina_plugins::vst2::Preset {
            kind: suhina_plugins::vst2::CHUNK_BANK,
            id: 7,
            version: 1,
            programs: 1,
            current: 0,
            parameters: 1,
            little_endian: false,
        },
        &0.25f32.to_le_bytes(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let restored = plugin.set_state(&bank);
    plugin.dispatcher = real;
    assert_eq!(restored, Ok(()));
    finished(plugin);
}

/// A plug-in that declares no editor is told so, with no window made for it.
#[test]
fn a_plug_in_without_the_editor_flag_opens_no_editor() {
    let (_link, mut plugin) = loaded(FIRST + 13);
    plugin.flags &= !super::effect_flags::HAS_EDITOR;
    let refused = plugin.open_editor(crate::window::Mode::Floating, None);
    assert_eq!(refused.err().as_deref(), Some("the plug-in has no editor"));
    finished(plugin);
}

/// The strings a plug-in asks for are written with their terminator and nothing past it: the
/// buffer's size is an assumption, and a shorter one must not be overrun.
#[test]
fn the_host_s_strings_stay_inside_a_short_buffer() {
    let (_link, plugin) = loaded(FIRST + 14);
    for op in [super::HostOp::GetVendorString, super::HostOp::GetProductString] {
        let mut buffer = [0xAAu8; 16];
        let ptr = buffer.as_mut_ptr().cast::<std::ffi::c_void>();
        assert_eq!(super::answer_at_entry(op.value(), ptr), 1);
        assert_eq!(&buffer[..7], b"Suhina\0");
        assert!(buffer[7..].iter().all(|b| *b == 0xAA), "{op:?}: nothing past the terminator");
        let mut buffer = [0xAAu8; 16];
        let ptr = buffer.as_mut_ptr().cast::<std::ffi::c_void>();
        assert_eq!(super::answer(&plugin.shared, false, op.value(), 0, 0, ptr, 0.0), 1);
        assert_eq!(&buffer[..7], b"Suhina\0");
        assert!(buffer[7..].iter().all(|b| *b == 0xAA), "{op:?}: nothing past the terminator");
    }
    finished(plugin);
}

/// The holds-serving state around a lifecycle dispatch: a host ask the plug-in makes inside
/// `MainsChanged`, `StartProcess` or `StopProcess` is answered from the lock-free slots and
/// the state is gone after — never the instance table's lock, which the audio thread takes
/// for a call it can wait for nothing.
#[test]
fn a_lifecycle_dispatch_serves_its_callback_without_the_table_lock() {
    static SERVING_HELD: AtomicBool = AtomicBool::new(false);
    // SAFETY: C ABI as the plug-in's dispatcher shape takes; the body reads this thread's
    // serving state and answers nothing.
    unsafe extern "C" fn serving_holding_dispatcher(
        _effect: *mut super::AEffect,
        _opcode: i32,
        _index: i32,
        _value: isize,
        _ptr: *mut std::ffi::c_void,
        _opt: f32,
    ) -> isize {
        let held = super::SERVING.with(|serving| serving.borrow().is_some());
        SERVING_HELD.store(held, Ordering::Relaxed);
        held as isize
    }

    let (_link, plugin) = loaded(FIRST + 8);
    // The mock's dispatcher takes the test's for the dispatch, and is put back over.
    // SAFETY: this process's own mock effect, changed and restored on this thread only.
    let mock = unsafe { (*plugin.effect).dispatcher };
    // SAFETY: as above, the test swopping its dispatcher in for the one dispatch.
    unsafe { (*plugin.effect).dispatcher = Some(serving_holding_dispatcher) };
    super::dispatch(&plugin.shared, plugin.effect, Some(serving_holding_dispatcher), super::EffectOp::MainsChanged, 1);
    // SAFETY: the test's own effect, put back as it was.
    unsafe { (*plugin.effect).dispatcher = mock };
    assert!(SERVING_HELD.load(Ordering::Relaxed), "the callback ran with the serving state up");
    assert!(
        !super::SERVING.with(|serving| serving.borrow().is_some()),
        "and the state is gone once the dispatch's own calls end"
    );
    // The real dispatcher is back before the plug-in finishes: its stop's own dispatches
    // (StopProcess, MainsChanged) go through the mock again.
    finished(plugin);
}
