// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's tests, over a real link in this process: the built-in mock through the whole
//! path a project's plug-in takes — load, blocks with events and automation, parameters,
//! state as a bank, the idle knob reported as an edit, a reconfigure. What these prove is
//! that the host and the mock agree; the *assumed* facts they both stand on are settled by
//! the probe and the ignored test against installed plug-ins (ADR 0022 decision 4).

use std::time::Duration;

use suhina_ipc::{Event, EventKind, Link, Outcome, Served};

use super::Vst2Plugin;
use crate::vst2::test_vst2::{EDIT_GAIN, LATENCY};

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

/// The knob the mock turns on its first idle call — the gesture a plug-in's editor makes —
/// comes out of the host as a parameter change, once.
#[test]
fn the_mock_s_knob_comes_out_as_a_parameter_change() {
    let (_link, mut plugin) = loaded(FIRST + 2);
    // The first tick serves the idle call the mock asked for; the knob it turns there is
    // reported by the same tick.
    assert_eq!(plugin.tick(), vec![(0, f64::from(EDIT_GAIN))]);
    // The knob turns once.
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
