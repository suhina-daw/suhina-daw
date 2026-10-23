// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine waits for each chain's plug-ins as they answer (#151), with real host processes:
//! two chains whose heavy plug-ins sit at different places take a block about as long as one
//! heavy plug-in, not two, and a hung plug-in in a third chain holds up neither.
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
        group: None,
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
                extensions: Extensions::default(),
            }],
        });
    }
    p
}

/// A renderer for `chains`, each a list of test plug-in specs, every plug-in in its own host
/// process; the controls (kept so the hosts run) and each plug-in's counts, in order.
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

/// Two chains of three plug-ins, a heavy one (20 ms a block) first in one and last in the other,
/// and a third chain whose plug-in hangs after its first blocks. A low sample rate gives the
/// blocks a long deadline (60 % of 128 ms). Stage by stage a block would take both heavy
/// plug-ins one after the other, 40 ms; each chain going on as its own plug-in answers takes
/// about 20 ms, and the hung plug-in costs its own chain only.
#[test]
fn heavy_plug_ins_in_different_chains_cost_one_heavy_plug_in_and_a_hung_one_costs_nothing_else() {
    const SR: u32 = 8000;
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
    for (n, count) in counts.iter().enumerate() {
        assert_eq!(count.read().0, 24, "plug-in {n} was given every block");
    }
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
    let (given, late) = counts[0].read();
    assert!(given == 8 && late >= 5, "the hung plug-in missed its blocks: {given} given, {late} late");
    for (n, count) in counts.iter().enumerate().skip(2) {
        let (given, late) = count.read();
        assert_eq!(given, 8, "plug-in {n} of the other chain was given every block");
        // Where the doorbell yields (macOS), a busy runner can make any plug-in late now and
        // then, so being on time is checked only where it sleeps, as above.
        if DOORBELL_SLEEPS {
            assert_eq!(late, 0, "plug-in {n} of the other chain was held up");
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
