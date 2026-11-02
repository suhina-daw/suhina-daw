// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine waits for each chain's plug-ins as they answer (#151), with real host processes: a
//! chain goes on while another chain's plug-in is still out, and a hung plug-in costs its own
//! chain the block. These tests count blocks and wait on a signal, so a slow machine gives the
//! same result as a fast one; how long blocks take is measured by the `#[ignore]`d tests, and
//! the engine's own tests check it on a manual clock (`crates/engine/src/chain_wait_tests.rs`).
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suhina_engine::{RenderInputs, RenderOptions};

use suhina_engine::{AudioPool, PluginSlotInfo, Plugins, Renderer, plugin_devices};
use suhina_model::*;
use suhina_plugins::{Launch, Plugin};
use suhina_session::{BlockCounts, Hosted};

const PATIENT: Duration = Duration::from_secs(20);
const FRAMES: usize = 1024;
/// A low sample rate gives the blocks a long deadline (60 % of 128 ms).
const SR: u32 = 8000;
/// Test plug-ins that signal each other, so they share a host process.
const RING: &str = "test:ring";
const AWAIT_RING: &str = "test:await-ring";
/// Whether a link's doorbell sleeps until it is rung (a futex on Linux, an event on Windows).
/// Elsewhere it yields for now (ADR 0006), so every waiting thread of the engine and the hosts
/// keeps running, and on a machine with few cores how long a block takes says more about the
/// scheduler than about the engine: the timings are checked only where the doorbell sleeps.
const DOORBELL_SLEEPS: bool = cfg!(any(target_os = "linux", windows));

fn start(spec: &str, sample_rate: u32) -> (Plugin, Hosted) {
    let launch = Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: spec.into(),
        channels: 2,
        max_frames: suhina_engine::MAX_BLOCK_FRAMES as u32,
        sample_rate: f64::from(sample_rate),
        timeout: PATIENT,
        group: [RING, AWAIT_RING].contains(&spec).then(|| "ring".to_string()),
        ara: None,
    };
    let (plugin, audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("{spec}: {e}"));
    (plugin, Hosted::new(audio))
}

fn plugin_device() -> Device {
    Device {
        name: "Test".into(),
        kind: DeviceKind::Clap { id: "x".into() },
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: Vec::new(),
        clip_host: false,
        extensions: Extensions::default(),
    }
}

/// One track per entry of `chains` (its number of plug-in devices), each playing a long note.
fn project(chains: &[usize]) -> Project {
    let mut p = Project {
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 64.0,
            lanes: vec![ContentLane {
                target: None,
                data: LaneData::Notes(vec![Note {
                    time: 0.0,
                    duration: 64.0,
                    key: 69,
                    velocity: 1.0,
                    release_velocity: 0.5,
                    channel: 0,
                    muted: false,
                }]),
            }],
            extensions: Extensions::default(),
            scale: None,
        }],
        ..Project::default()
    };
    for (n, &devices) in chains.iter().enumerate() {
        let id = n as u32 + 1;
        p.tracks.push(Track {
            id: TrackId(id),
            name: format!("T{id}"),
            color: None,
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(id)),
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        });
        p.channels.push(Channel {
            id: ChannelId(id),
            name: format!("C{id}"),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices: (0..devices).map(|_| plugin_device()).collect(),
            extensions: Extensions::default(),
            automation: Vec::new(),
        });
        p.arrangement.lanes.push(ArrangementLane {
            id: Default::default(),
            name: format!("l{id}"),
            track: Some(TrackId(id)),
            clips: vec![Clip {
                id: Default::default(),
                content: ContentId(1),
                start: 0.0,
                length: 64.0,
                offset: 0.0,
                loop_range: None,
                enabled: true,
                fade_in: 0.0,
                fade_out: 0.0,
                fade_in_curve: Default::default(),
                fade_out_curve: Default::default(),
                follow_action: Default::default(),
                launch: Default::default(),
                color: None,
                ara: false,
                extensions: Extensions::default(),
            }],
        });
    }
    p
}

/// A renderer for `chains`, each a list of test plug-in specs, every plug-in in its own host
/// process but [`RING`] and [`AWAIT_RING`]; the controls (kept so the hosts run) and each plug-in's counts, in order.
fn start_chains(chains: &[Vec<&str>], sample_rate: u32) -> (Renderer, Vec<Plugin>, Vec<Arc<BlockCounts>>) {
    let lengths: Vec<usize> = chains.iter().map(|c| c.len()).collect();
    let p = project(&lengths);
    let mut plugins = Plugins::new();
    let mut controls = Vec::new();
    let mut counts = Vec::new();
    for (found, spec) in plugin_devices(&p).into_iter().zip(chains.iter().flat_map(|c| c.iter())) {
        let (control, hosted) = start(spec, sample_rate);
        counts.push(hosted.counts());
        plugins.insert(found.key, PluginSlotInfo::default(), Box::new(hosted));
        controls.push(control);
    }
    let renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        sample_rate,
        RenderOptions::PLAYING,
    )
    .unwrap();
    (renderer, controls, counts)
}

/// Runs `blocks` blocks of [`FRAMES`], one straight after the other.
fn run(renderer: &mut Renderer, blocks: usize) {
    let mut block = vec![0.0f32; FRAMES * 2];
    for _ in 0..blocks {
        renderer.process(&mut block);
    }
}

/// Plays `blocks` blocks of [`FRAMES`] in real time, one every block's length; each block's
/// time, sorted.
fn play(renderer: &mut Renderer, blocks: usize, sample_rate: u32) -> Vec<Duration> {
    let period = Duration::from_secs_f64(FRAMES as f64 / f64::from(sample_rate));
    let mut block = vec![0.0f32; FRAMES * 2];
    let mut times = Vec::with_capacity(blocks);
    for _ in 0..blocks {
        let started = Instant::now();
        renderer.process(&mut block);
        times.push(started.elapsed());
        std::thread::sleep(period.saturating_sub(started.elapsed()));
    }
    times.sort();
    times
}

/// A plug-in that stays out until the last plug-in of another chain has had the block: a chain
/// that waited for the other chains' plug-ins would never get there. Offline every answer is
/// waited for, so nothing here depends on how fast the machine is: were the chain held up, the
/// plug-in out would use up `OFFLINE_TIMEOUT` and count as late.
#[test]
fn a_chain_goes_on_while_another_chains_plug_in_is_still_out() {
    const BLOCKS: u32 = 4;
    let (mut renderer, _controls, counts) = start_chains(
        &[vec![AWAIT_RING, "test:busy:200", "test:busy:200"], vec!["test:busy:200", "test:busy:200", RING]],
        SR,
    );
    renderer.set_offline(true);
    run(&mut renderer, BLOCKS as usize);
    for (n, count) in counts.iter().enumerate() {
        assert_eq!(count.read(), (BLOCKS, 0), "plug-in {n} was given every block and answered it");
    }
}

/// A plug-in that hangs after its first blocks misses every block after them, however fast the
/// machine is, and costs nothing but the sound of those: the next plug-in of its chain and the
/// plug-ins of the other chain are still given every block.
#[test]
fn a_hung_plug_in_costs_its_chain_the_block_and_the_others_are_still_given_theirs() {
    const BLOCKS: u32 = 8;
    const ANSWERS: u32 = 2;
    let (mut renderer, _controls, counts) = start_chains(
        &[vec!["test:hang-after:2", "test:busy:200"], vec!["test:busy:200", "test:busy:5000", "test:busy:200"]],
        SR,
    );
    run(&mut renderer, BLOCKS as usize);
    for (n, count) in counts.iter().enumerate() {
        assert_eq!(count.read().0, BLOCKS, "plug-in {n} was given every block");
    }
    let late = counts[0].read().1;
    assert!(late >= BLOCKS - ANSWERS, "the hung plug-in answers only {ANSWERS} blocks: {late} late");
}

/// How long a block takes on the real clock, which a shared machine's scheduler can stretch at
/// any time, so this says something only on an idle one. Two chains of three plug-ins, a heavy
/// one (20 ms a block) first in one and last in the other, then a chain whose plug-in hangs after
/// its first blocks. Stage by stage a block would take both heavy plug-ins one after the other,
/// 40 ms; each chain going on as its own plug-in answers takes about 20 ms, and the hung plug-in
/// costs its own chain only. Run with
/// `cargo test -p suhina-plugin-host --release --test chain_wait block_times -- --ignored`.
#[test]
#[ignore = "bounds on elapsed time, for an idle machine"]
fn block_times_with_heavy_and_hung_plug_ins() {
    let (mut renderer, _controls, counts) = start_chains(
        &[
            vec!["test:busy:20000", "test:busy:200", "test:busy:200"],
            vec!["test:busy:200", "test:busy:200", "test:busy:20000"],
        ],
        SR,
    );
    // Settle (the hosts' first blocks), then measure.
    play(&mut renderer, 4, SR);
    let times = play(&mut renderer, 20, SR);
    let median = times[times.len() / 2];
    assert!(median >= Duration::from_millis(20), "{times:?}");
    if DOORBELL_SLEEPS {
        assert!(
            median < Duration::from_millis(34),
            "a block took {median:?}, as long as both heavy plug-ins: {times:?}"
        );
        for (n, count) in counts.iter().enumerate() {
            assert_eq!(count.read().1, 0, "plug-in {n} was late");
        }
    }

    let (mut renderer, _controls, counts) = start_chains(
        &[vec!["test:hang-after:2", "test:busy:200"], vec!["test:busy:200", "test:busy:5000", "test:busy:200"]],
        SR,
    );
    let deadline = Duration::from_secs_f64(FRAMES as f64 / f64::from(SR) * 0.6);
    let times = play(&mut renderer, 8, SR);
    let longest = times[times.len() - 1];
    // Past the deadline, the hung plug-in's chain hands its next plug-in the block and takes
    // its miss at once; a yielding doorbell may wait a scheduler slice or two for that.
    let margin = Duration::from_millis(if DOORBELL_SLEEPS { 15 } else { 40 });
    assert!(longest < deadline + margin, "{longest:?} past the {deadline:?} deadline");
    // Where the doorbell yields (macOS), any plug-in can be late now and then.
    if DOORBELL_SLEEPS {
        for (n, count) in counts.iter().enumerate().skip(2) {
            assert_eq!(count.read().1, 0, "plug-in {n} of the other chain was held up");
        }
    }
}

/// Block times with three chains whose heavy plug-ins (3 ms a block) sit at different stages,
/// at 48 kHz and 1024 frames (deadline 12.8 ms), every plug-in in its own process; prints the
/// average, the 99th percentile, the longest and the plug-ins' longest round trip
/// (`longest_us`). Run with
/// `cargo test -p suhina-plugin-host --release --test chain_wait measure -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_chains_waiting_for_their_own_plug_ins() {
    const SR: u32 = 48_000;
    const BLOCKS: usize = 1000;
    let (mut renderer, _controls, counts) = start_chains(
        &[
            vec!["test:busy:3000", "test:busy:300", "test:busy:300"],
            vec!["test:busy:300", "test:busy:3000", "test:busy:300"],
            vec!["test:busy:300", "test:busy:300", "test:busy:3000"],
        ],
        SR,
    );
    play(&mut renderer, 20, SR);
    let times = play(&mut renderer, BLOCKS, SR);
    let mean = times.iter().sum::<Duration>() / BLOCKS as u32;
    let late: u32 = counts.iter().map(|c| c.read().1).sum();
    let longest_us = counts.iter().map(|c| c.longest_us()).max().unwrap_or(0);
    println!(
        "{BLOCKS} blocks: average {mean:?}, 99 % {:?}, longest {:?}; late blocks {late}; longest round trip {longest_us} µs",
        times[BLOCKS * 99 / 100],
        times[BLOCKS - 1]
    );
}
