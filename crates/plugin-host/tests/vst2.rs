// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The VST2 host through the engine's whole boundary, with the built-in mock: a host process
//! started, the instance loaded over the control protocol, blocks with events and automation
//! through the link, parameters, state as the standard bank, and the edit a plug-in reports
//! of itself. Everything here is behind the `vst2` feature, as the host is (ADR 0022); the
//! mock stands for an installed plug-in, so these prove the boundary, and the *assumed*
//! interface facts are settled by the ignored probe test.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions
#![cfg(feature = "vst2")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use suhina_ipc::{Event, EventKind, Outcome};
use suhina_plugins::{Launch, Plugin};

/// How long anything waits for the host process.
const PATIENT: Duration = Duration::from_secs(20);
/// The gain the mock's idle knob turns it to, which it plays with afterwards.
const EDIT_GAIN: f32 = 0.125;
/// The latency the mock reports, in the field the host assumes holds it.
const LATENCY: u32 = 32;

/// The mock, loaded into a host process of its own.
fn start() -> (Plugin, suhina_plugins::PluginAudio) {
    let launch = Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: "test:vst2".into(),
        channels: 2,
        max_frames: 256,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: None,
        ara: None,
    };
    let (plugin, audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("{e}"));
    (plugin, audio)
}

/// The mock's idle knob turns once, shortly after it loads (its host's main loop serves the
/// idle call it asked for). Every test that reads its gain waits for the turn first, so what
/// they see is deterministic: the gain the mock plays with afterwards is [`EDIT_GAIN`].
fn knob_turned(plugin: &mut Plugin) {
    let deadline = Instant::now() + PATIENT;
    while plugin.take_param_edits() != vec![(0, f64::from(EDIT_GAIN))] {
        assert!(Instant::now() < deadline, "the knob's edit never came");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// One block through the link, with `events` at their frames.
fn block(audio: &mut suhina_plugins::PluginAudio, input: &[f32], events: &[Event]) -> Vec<f32> {
    let mut output = vec![0.0f32; input.len()];
    let mut all: Vec<Event> =
        vec![Event { frame: 0, note: -1, kind: EventKind::Transport { playing: true, position: 0 } }];
    all.extend(events.iter().copied());
    assert_eq!(audio.process_with_events(input, &all, &mut output, PATIENT), Outcome::Done, "the block came back");
    output
}

/// The mock loads over the boundary, plays a block with its gain, a note at its frame, and
/// follows a parameter the block carries.
#[test]
fn the_mock_runs_through_the_boundary() {
    let (mut plugin, mut audio) = start();
    assert_eq!(plugin.latency(), LATENCY, "the latency the assumed field holds");
    knob_turned(&mut plugin);

    let params = plugin.params().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "Gain");
    assert_eq!(params[0].value, f64::from(EDIT_GAIN), "the knob's gain, after it turned");

    let input: Vec<f32> = (0..512).map(|i| (i % 24) as f32 / 24.0).collect();
    let output = block(&mut audio, &input, &[]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * EDIT_GAIN).abs() < 1e-6, "{out} vs {sample}");
    }

    // A note at its frame, as `test:vst3` plays it, so either format's events path is
    // audible through the same boundary.
    let note = Event { frame: 10, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } };
    let output = block(&mut audio, &input, &[note]);
    assert!((output[10 * 2] - input[10 * 2] * EDIT_GAIN - 1.0).abs() < 1e-6, "{}", output[20]);
    assert!((output[11 * 2] - input[11 * 2] * EDIT_GAIN).abs() < 1e-6, "only the note's frame");

    // A parameter the block carries (automation) reaches the plug-in before its audio.
    let change = Event { frame: 0, note: -1, kind: EventKind::Param { id: 0, value: 0.9 } };
    let output = block(&mut audio, &input, &[change]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * 0.9).abs() < 1e-6, "{out} vs {sample}");
    }
}

/// A state saved as the standard bank comes back through a second instance, both bank forms:
/// the plug-in's own chunk, and a program bank's values set as parameters.
#[test]
fn a_state_round_trips_over_the_boundary() {
    let (mut plugin, _audio) = start();
    knob_turned(&mut plugin);
    plugin.set_param(0, 0.25).unwrap_or_else(|e| panic!("{e}"));
    let state = plugin.save_state().unwrap_or_else(|e| panic!("{e}"));
    let bank = state.data.clone();
    assert_eq!(&bank[..4], b"CcnK", "the standard bank");
    assert_eq!(&bank[8..12], b"FBCh", "the mock has a chunk");
    assert_eq!(suhina_plugins::vst2::chunk(&bank), Some(0.25f32.to_le_bytes().to_vec().as_slice()));

    let (mut reopened, _audio) = start();
    knob_turned(&mut reopened);
    reopened.load_state(state).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.25);

    // A program bank, as an imported project holds one.
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
    let program_bank = suhina_plugins::vst2::bank(&preset, &buffer).unwrap_or_else(|e| panic!("{e}"));
    reopened.load_state(program_bank).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.75);
}

/// The knob the mock turns on its first idle call comes back over the boundary as a
/// parameter change, as an editor's move does.
#[test]
fn the_mock_s_edit_comes_back_over_the_boundary() {
    let (mut plugin, _audio) = start();
    let deadline = Instant::now() + PATIENT;
    loop {
        let edits = plugin.take_param_edits();
        if edits == vec![(0, f64::from(EDIT_GAIN))] {
            break;
        }
        assert!(edits.is_empty(), "nothing before the knob: {edits:?}");
        assert!(Instant::now() < deadline, "the knob's edit never came");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The knob turns once.
    std::thread::sleep(Duration::from_millis(50));
    assert!(plugin.take_param_edits().is_empty());
}
