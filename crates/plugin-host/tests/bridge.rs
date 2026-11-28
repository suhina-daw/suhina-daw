// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge for 32-bit Windows plug-ins (docs/spec/plugin-hosting.md › 32-bit plug-ins on
//! Windows), across the two bitnesses: this 64-bit test drives the 32-bit host as the engine
//! does, over the control protocol, the shared memory, the doorbells and the events in a block,
//! with the test plug-ins built into the host. The 32-bit host is named by
//! `SUHINA_TEST_BRIDGE_HOST` (`python scripts/bridge_host.py` builds it); without it the tests
//! pass without running, as CI's other jobs have none.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::Duration;

use suhina_ipc::{Event, EventKind, Outcome};
use suhina_plugins::{Launch, Plugin, PluginAudio};

const FRAMES: usize = 64;
const PATIENT: Duration = Duration::from_secs(20);

/// The 32-bit host, when this run has one.
fn bridge() -> Option<PathBuf> {
    let host = PathBuf::from(std::env::var_os("SUHINA_TEST_BRIDGE_HOST")?);
    assert!(host.is_file(), "SUHINA_TEST_BRIDGE_HOST names no file: {}", host.display());
    Some(host)
}

fn start(host: PathBuf, plugin: &str, group: Option<&str>) -> (Plugin, PluginAudio) {
    let launch = Launch {
        host,
        plugin: plugin.into(),
        channels: 2,
        max_frames: FRAMES as u32,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: group.map(str::to_string),
        ara: None,
    };
    Plugin::start(launch).unwrap_or_else(|e| panic!("{plugin} in the 32-bit host: {e}"))
}

/// One block of `input` with the transport playing and `events`, as the engine sends it.
fn block(audio: &mut PluginAudio, input: &[f32], events: &[Event]) -> Vec<f32> {
    let mut output = vec![9.0f32; input.len()];
    let mut all = vec![Event { frame: 0, note: -1, kind: EventKind::Transport { playing: true, position: 0 } }];
    all.extend(events.iter().copied());
    assert_eq!(audio.process_with_events(input, &all, &mut output, PATIENT), Outcome::Done, "the block came back");
    output
}

/// Audio both ways and a state round trip: the gain the state sets is the gain the 32-bit
/// process applies, and two instances in one group share the 32-bit process.
#[test]
fn audio_and_state_cross_into_the_32_bit_host() {
    let Some(host) = bridge() else { return };
    let (mut first, mut first_audio) = start(host.clone(), "test:gain", Some("bridge"));
    let (second, mut second_audio) = start(host, "test:gain", Some("bridge"));
    assert_eq!(first.process_id(), second.process_id(), "one 32-bit process for the group");
    first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    let input: Vec<f32> = (0..FRAMES * 2).map(|i| (i as f32 * 0.37).sin()).collect();
    for _ in 0..10 {
        let out = block(&mut first_audio, &input, &[]);
        assert!(out.iter().zip(&input).all(|(o, i)| (o - i * 0.25).abs() < 1e-6), "gain 0.25: {out:?}");
        let out = block(&mut second_audio, &input, &[]);
        assert!(out.iter().zip(&input).all(|(o, i)| (o - i * 0.5).abs() < 1e-6), "gain 0.5: {out:?}");
    }
    assert_eq!(first.save_state().unwrap().data, 0.25f32.to_le_bytes());
    assert!(first.ping());
}

/// Each format's test plug-in loads in the 32-bit host, runs blocks with a note in them, and
/// keeps its state: what one instance saved, a fresh one loads and saves again unchanged.
#[test]
fn every_format_runs_and_keeps_its_state_in_the_32_bit_host() {
    let Some(host) = bridge() else { return };
    let note = [Event::note_on(8, 0, 60, 0.8)];
    let input = vec![0.5f32; FRAMES * 2];
    for spec in ["test:clap", "test:vst3", "test:vst2"] {
        let (mut plugin, mut audio) = start(host.clone(), spec, None);
        for i in 0..4 {
            let out = block(&mut audio, &input, if i == 0 { &note } else { &[] });
            assert!(out.iter().all(|s| s.is_finite() && *s != 9.0), "{spec} wrote its block: {out:?}");
        }
        plugin.params().unwrap_or_else(|e| panic!("{spec} parameters: {e}"));
        let saved = plugin.save_state().unwrap();
        let (mut fresh, mut fresh_audio) = start(host.clone(), spec, None);
        fresh.load_state(saved.data.clone()).unwrap();
        block(&mut fresh_audio, &input, &[]);
        assert_eq!(fresh.save_state().unwrap().data, saved.data, "{spec} state round trip");
    }
}
