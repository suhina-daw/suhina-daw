// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Play and stop pressed quickly with a plug-in instrument (#506), with real host processes: an
//! instrument that takes longer over the blocks that bring notes (a start's note-ons, a stop's
//! note-offs and all-notes-off), as a synthesizer does when it starts or resets voices, must
//! not cost the blocks after it, nor the notes they bring.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suhina_engine::{RenderInputs, RenderOptions};

use suhina_engine::{AudioPool, LaunchCommand, PluginSlotInfo, Plugins, Renderer, plugin_devices};
use suhina_model::*;
use suhina_plugins::{Launch, Plugin};
use suhina_session::{BlockCounts, Hosted};

const PATIENT: Duration = Duration::from_secs(20);
/// Whether a link's doorbell sleeps until it is rung (a futex on Linux, an event on Windows).
/// Elsewhere it yields for now (ADR 0006), so every waiting thread of the engine and the hosts
/// keeps running, and on a machine with few cores how late a block is says more about the
/// scheduler than about the engine: the timings are checked only where the doorbell sleeps, as
/// in `chain_wait.rs`.
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

/// One MIDI track per instrument, `tracks` of them, each with a plug-in instrument and playing
/// one long note from the start: the only notes a plug-in gets are a start's note-on and a
/// stop's note-off and all-notes-off, on every track at the same frame.
fn project(tracks: u32) -> Project {
    let notes = vec![Note {
        time: 0.0,
        duration: 64.0,
        key: 60,
        velocity: 1.0,
        release_velocity: 0.5,
        channel: 0,
        muted: false,
    }];
    let mut p = Project {
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 64.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        ..Project::default()
    };
    for id in 1..=tracks {
        p.tracks.push(Track {
            id: TrackId(id),
            name: format!("Synth {id}"),
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
            name: format!("Synth {id}"),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices: vec![Device {
                name: "Synth".into(),
                kind: DeviceKind::Clap { id: "x".into() },
                enabled: true,
                state: None,
                params: Vec::new(),
                automation: Vec::new(),
                extensions: Extensions::default(),
            }],
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

/// A stopped renderer with one track per instrument of `specs`, each in its own host process;
/// the controls (kept so the hosts run) and each plug-in's counts, in order.
fn renderer(specs: &[&str], sample_rate: u32) -> (Renderer, Vec<Plugin>, Vec<Arc<BlockCounts>>) {
    let p = project(specs.len() as u32);
    let mut plugins = Plugins::new();
    let mut controls = Vec::new();
    let mut counts = Vec::new();
    for (found, spec) in plugin_devices(&p).into_iter().zip(specs) {
        let (control, hosted) = start(spec, sample_rate);
        counts.push(hosted.counts());
        plugins.insert(found.key, PluginSlotInfo { instrument: true, latency: 0 }, Box::new(hosted));
        controls.push(control);
    }
    let renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        sample_rate,
        RenderOptions::STOPPED,
    )
    .unwrap();
    (renderer, controls, counts)
}

/// What playing with play and stop pressed every few blocks did.
#[derive(Debug)]
struct Run {
    /// Per plug-in, blocks it was given.
    given: Vec<u32>,
    /// Per plug-in, blocks it did not answer in time (sent late, or not sent).
    late: Vec<u32>,
    /// Blocks of the output that were silent: every instrument's sound dropped out.
    silent: usize,
    /// How long `process` took, sorted.
    times: Vec<Duration>,
}

impl Run {
    fn percentile(&self, share: f64) -> Duration {
        let index = ((self.times.len() as f64 - 1.0) * share).round() as usize;
        self.times.get(index).copied().unwrap_or_default()
    }

    fn late(&self) -> u32 {
        self.late.iter().sum()
    }
}

/// Plays `blocks` blocks of `frames` in real time, one every block's length, pressing play or
/// stop at the start of every `every`-th block.
fn toggle(
    renderer: &mut Renderer,
    counts: &[Arc<BlockCounts>],
    frames: usize,
    sample_rate: u32,
    blocks: usize,
    every: usize,
) -> Run {
    let period = Duration::from_secs_f64(frames as f64 / f64::from(sample_rate));
    let mut block = vec![0.0f32; frames * 2];
    let mut times = Vec::with_capacity(blocks);
    let before: Vec<(u32, u32)> = counts.iter().map(|c| c.read()).collect();
    let mut silent = 0;
    let mut playing = false;
    let mut next = Instant::now();
    for n in 0..blocks {
        if n % every == 0 {
            renderer.handle(if playing { LaunchCommand::Stop } else { LaunchCommand::Play });
            playing = !playing;
        }
        let started = Instant::now();
        renderer.process(&mut block);
        times.push(started.elapsed());
        if block.iter().all(|s| s.abs() < 0.01) {
            silent += 1;
        }
        next += period;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    times.sort();
    let now: Vec<(u32, u32)> = counts.iter().map(|c| c.read()).collect();
    let given = now.iter().zip(&before).map(|(now, before)| now.0 - before.0).collect();
    let late = now.iter().zip(&before).map(|(now, before)| now.1 - before.1).collect();
    Run { given, late, silent, times }
}

/// An instrument that takes longer than a whole block over every block with notes, with play
/// and stop pressed every third block: the block it is late with is silent, and that is all.
/// The block after it goes out as soon as the plug-in has caught up, within that block's own
/// deadline, rather than being refused at once and lost too. Low sample rate, so the blocks
/// are long (128 ms) and the scheduler's slices small beside them.
#[test]
fn a_plug_in_late_with_a_start_or_a_stop_costs_that_block_only() {
    const SR: u32 = 8000;
    const FRAMES: usize = 1024;
    // 140 ms: past the block's deadline (76.8 ms) and its end (128 ms), and over early in the
    // next block.
    let (mut renderer, _controls, counts) = renderer(&["test:burst:140000"], SR);
    toggle(&mut renderer, &counts, FRAMES, SR, 3, 1000);
    let run = toggle(&mut renderer, &counts, FRAMES, SR, 18, 3);
    assert_eq!(run.given, [18], "the plug-in was given every block: {run:?}");
    assert!(run.silent <= run.late() as usize, "only a late block may be silent: {run:?}");
    if DOORBELL_SLEEPS {
        // Every press brings notes (a start's note-on at beat 0, a stop's note-off and
        // all-notes-off): six presses, one late block each.
        assert!(run.late() <= 6, "a press cost more than its own block: {run:?}");
        assert!(run.late() >= 5, "the plug-in's bursts are past the deadline: {run:?}");
        let deadline = Duration::from_secs_f64(FRAMES as f64 / f64::from(SR) * 0.6);
        assert!(
            run.percentile(1.0) < deadline + Duration::from_millis(15),
            "a block waited past its deadline: {run:?}"
        );
    }
}

/// Two tracks whose instruments get their notes at the same frame: one takes longer than a
/// block over them, the other almost no time. While the chain of the first waits for its
/// plug-in to catch up, the second goes on: it is never late, and the block still ends by the
/// deadline.
#[test]
fn a_plug_in_catching_up_holds_up_no_other_track() {
    const SR: u32 = 8000;
    const FRAMES: usize = 1024;
    let (mut renderer, _controls, counts) = renderer(&["test:burst:140000", "test:burst:200"], SR);
    toggle(&mut renderer, &counts, FRAMES, SR, 3, 1000);
    let run = toggle(&mut renderer, &counts, FRAMES, SR, 18, 3);
    assert_eq!(run.given, [18, 18], "both plug-ins were given every block: {run:?}");
    if DOORBELL_SLEEPS {
        assert!(run.late[0] <= 6, "a press cost the slow instrument more than its own block: {run:?}");
        assert_eq!(run.silent, 0, "the other instrument sounds through every block: {run:?}");
        assert_eq!(run.late[1], 0, "the other instrument was held up: {run:?}");
        let deadline = Duration::from_secs_f64(FRAMES as f64 / f64::from(SR) * 0.6);
        assert!(run.percentile(1.0) < deadline + Duration::from_millis(15), "{run:?}");
    }
}

/// Play and stop pressed every few blocks with instruments of different weights, at 48 kHz and
/// 256 frames (5.3 ms a block, the plug-ins' deadline 3.2 ms); prints, for each, the blocks the
/// plug-in was late with, the silent blocks, and how long a block took. Run with
/// `cargo test -p suhina-plugin-host --release --test quick_play_stop measure -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_quick_play_and_stop() {
    const SR: u32 = 48_000;
    const FRAMES: usize = 256;
    const BLOCKS: usize = 2000;
    let one = |spec: &'static str| vec![spec];
    let setups: [(&str, Vec<&str>); 7] = [
        ("one instrument, 0 ms", one("test:burst:0")),
        ("one instrument, 1 ms", one("test:burst:1000")),
        ("one instrument, 2.5 ms", one("test:burst:2500")),
        ("one instrument, 4 ms", one("test:burst:4000")),
        ("one instrument, 6 ms", one("test:burst:6000")),
        ("four instruments, 1 ms each", vec!["test:burst:1000"; 4]),
        (
            "four instruments, 6 ms, 1 ms, 1 ms, 0.2 ms",
            vec!["test:burst:6000", "test:burst:1000", "test:burst:1000", "test:burst:200"],
        ),
    ];
    for (name, specs) in setups {
        for every in [4, 20] {
            let (mut renderer, _controls, counts) = renderer(&specs, SR);
            toggle(&mut renderer, &counts, FRAMES, SR, 20, 1000);
            let run = toggle(&mut renderer, &counts, FRAMES, SR, BLOCKS, every);
            println!(
                "{name}, a press every {every} blocks ({} presses): late {:?}, silent {}; block median {:?}, 99 % {:?}, longest {:?}",
                BLOCKS / every,
                run.late,
                run.silent,
                run.percentile(0.5),
                run.percentile(0.99),
                run.percentile(1.0),
            );
        }
    }
}
