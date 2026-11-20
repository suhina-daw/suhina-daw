// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{PluginProcessor, PluginSlotInfo, Plugins, RenderInputs, RenderOptions};
use suhina_model::{ChannelId, Device, DeviceKind};
use suhina_test_support::ProjectBuilder;

const RATE: u32 = 8_000;

fn renderer(devices: &[&str]) -> Renderer {
    let project = ProjectBuilder::new()
        .audio_track(1, "Probe")
        .with(|p| {
            p.channels[0].devices =
                devices.iter().map(|id| Device::new(*id, DeviceKind::Builtin((*id).into()))).collect();
        })
        .build();
    let mut renderer = Renderer::new(&project, RATE, suhina_device_registry::factory()).unwrap();
    renderer.set_offline(true);
    renderer
}

fn probe(frames: usize, lead_ms: u32, devices: &[&str]) -> Probe {
    Probe::new(renderer(devices), TrackId(1), Config { block_frames: frames, lead_ms }).unwrap()
}

fn signal(at: u64, out: &mut [f32]) {
    for (offset, pair) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let phase = (at + offset as u64) % 251;
        let value = (phase as f32 - 125.0) / 512.0;
        pair.copy_from_slice(&[value, -value * 0.5]);
    }
}

fn constant(_: u64, out: &mut [f32]) {
    out.fill(0.25);
}

/// The ordinary synchronous chain is the oracle, not a second probe with another setting.
fn ordinary(chain: &mut Chain, at: u64, out: &mut [f32]) {
    let frames = out.len() / 2;
    chain.begin(frames, None);
    signal(at, chain.input(frames));
    chain.automate_mix(at, frames, true);
    chain.complete(frames, &ProcessContext { tempo: 120.0, beat: at as f64 / 4000.0, playing: true });
    out.fill(0.0);
    chain.mix(out, &mut [], frames);
}

#[test]
fn switching_preserves_every_sample_of_the_ordinary_stateful_chain() {
    for frames in [1, 31, 64, 257, 1024] {
        for lead in [50, 100, 200] {
            let mut probe = probe(frames, lead, &["utility", "delay", "reverb"]);
            let mut renderer = renderer(&["utility", "delay", "reverb"]);
            let mut plain = renderer.graph.chains.remove(0);
            let mut actual = vec![0.0; frames * 2];
            let mut expected = actual.clone();
            // Enough blocks to fill and drain even a one-frame, 200 ms queue.
            let run = probe.target_blocks + 8;
            for block in 0..run * 4 {
                if block == 2 || block == run * 2 {
                    probe.set_path(Path::Ahead).unwrap();
                }
                if block == run || block == run * 3 {
                    probe.set_path(Path::Live).unwrap();
                }
                // The same mixer gestures at the heard position, including while ahead.
                if block == run / 2 {
                    probe.set_volume(0.3);
                    plain.set_live_volume(0.3);
                    probe.set_pan(-0.6);
                    plain.set_live_pan(-0.6);
                }
                let status = probe.process(&mut actual, true, &mut signal).unwrap();
                ordinary(&mut plain, (block * frames) as u64, &mut expected);
                assert!(
                    actual.iter().zip(&expected).all(|(a, b)| a.to_bits() == b.to_bits()),
                    "{frames} frames, {lead} ms, block {block}"
                );
                assert_eq!(status.rendered_frames - status.played_frames, status.queued_frames as u64);
                assert!(status.rendered_blocks <= 2);
            }
            assert_eq!(probe.status().state, State::Live);
        }
    }
}

#[test]
fn budget_and_reversed_requests_keep_one_unbroken_source_stream() {
    let mut probe = probe(64, 50, &["utility"]);
    let mut next = 0;
    let mut out = [0.0; 128];
    probe.set_path(Path::Ahead).unwrap();
    for block in 0..48 {
        match block {
            5 | 23 => probe.set_path(Path::Live).unwrap(),
            7 | 25 => probe.set_path(Path::Ahead).unwrap(),
            _ => {}
        }
        let before = probe.status().queued_frames;
        let spare = block >= 4 && block % 3 != 0;
        let status = probe
            .process(&mut out, spare, &mut |at, audio| {
                assert_eq!(at, next, "no repeated or skipped source frames");
                next += 64;
                signal(at, audio);
            })
            .unwrap();
        assert!(status.rendered_blocks <= if spare { 2 } else { 1 });
        if !spare {
            assert!(status.queued_frames <= before);
        }
    }
    assert_eq!(probe.status().state, State::Ahead);
    assert_eq!(next, probe.status().rendered_frames);
}

#[test]
fn a_device_change_waits_for_the_queue_but_a_fader_move_does_not() {
    for lead in [50, 100, 200] {
        let mut device = probe(64, lead, &["utility"]);
        let mut fader = probe(64, lead, &["utility"]);
        device.set_path(Path::Ahead).unwrap();
        fader.set_path(Path::Ahead).unwrap();
        let mut a = [0.0; 128];
        let mut b = a;
        for _ in 0..device.target_blocks + 2 {
            device.process(&mut a, true, &mut constant).unwrap();
            fader.process(&mut b, true, &mut constant).unwrap();
        }
        let queued = device.status().queued_frames;
        device.set_device_param(0, 0, -12.0);
        device.set_path(Path::Live).unwrap();
        fader.set_volume(0.25);
        let mut first_device = None;
        let mut first_fader = None;
        for block in 0..device.target_blocks + 2 {
            device.process(&mut a, false, &mut constant).unwrap();
            fader.process(&mut b, false, &mut constant).unwrap();
            for (frame, (a, b)) in a.as_chunks::<2>().0.iter().zip(b.as_chunks::<2>().0.iter()).enumerate() {
                let at = block * 64 + frame;
                if a[0] != 0.25 && first_device.is_none() {
                    first_device = Some(at);
                }
                if b[0] != 0.25 && first_fader.is_none() {
                    first_fader = Some(at);
                }
            }
        }
        assert_eq!(first_device, Some(queued));
        assert!(first_fader.is_some_and(|frame| frame < 64));
        assert_eq!(device.status().state, State::Live);
    }
}

struct Delay {
    samples: Vec<f32>,
    at: usize,
}

impl PluginProcessor for Delay {
    fn process(
        &mut self,
        out: &mut [f32],
        _: &[(usize, suhina_device_api::NoteEvent)],
        _: std::time::Duration,
    ) -> bool {
        for sample in out {
            std::mem::swap(sample, &mut self.samples[self.at]);
            self.at = (self.at + 1) % self.samples.len();
        }
        true
    }
}

fn delayed_renderer() -> Renderer {
    let project = ProjectBuilder::new()
        .audio_track(1, "Probe")
        .audio_track(2, "Longer")
        .with(|p| {
            for channel in &mut p.channels {
                channel.devices.push(Device::new("Delay", DeviceKind::Clap { id: "test:path-delay".into() }));
            }
        })
        .build();
    let mut plugins = Plugins::new();
    for device in crate::plugin_devices(&project) {
        let latency = if device.channel == ChannelId(1) { 17 } else { 43 };
        plugins.insert(
            device.key,
            PluginSlotInfo { instrument: false, latency },
            Box::new(Delay { samples: vec![0.0; latency as usize * 2], at: 0 }),
        );
    }
    Renderer::build(
        &project,
        RenderInputs::new(suhina_device_registry::factory()).with_plugins(plugins),
        RATE,
        RenderOptions::PLAYING,
    )
    .unwrap()
}

#[test]
fn plugin_state_and_compensation_do_not_move_at_a_switch() {
    let mut probe = Probe::new(delayed_renderer(), TrackId(1), Config { block_frames: 31, lead_ms: 50 }).unwrap();
    let mut renderer = delayed_renderer();
    let mut plain = renderer.graph.chains.remove(0);
    assert_eq!(probe.latency(), (17, 26));
    let mut a = [0.0; 62];
    let mut b = a;
    for block in 0..120 {
        if block == 3 || block == 70 {
            probe.set_path(Path::Ahead).unwrap();
        }
        if block == 40 || block == 95 {
            probe.set_path(Path::Live).unwrap();
        }
        probe.process(&mut a, true, &mut signal).unwrap();
        ordinary(&mut plain, block * 31, &mut b);
        assert!(a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()));
        assert_eq!(probe.latency(), (17, 26));
    }
}

#[test]
fn processing_and_switching_allocate_and_free_nothing() {
    let mut probe = probe(64, 50, &["utility", "delay"]);
    let mut out = [0.0; 128];
    let counts = allocation_counter::measure(|| {
        for block in 0..80 {
            if block == 0 || block == 40 {
                probe.set_path(Path::Ahead).unwrap();
            }
            if block == 20 || block == 60 {
                probe.set_path(Path::Live).unwrap();
            }
            probe.process(&mut out, true, &mut signal).unwrap();
        }
    });
    assert_eq!(counts.count_total, 0);
    assert_eq!(counts.count_current, 0);
    assert_eq!(counts.bytes_total, 0);
    assert_eq!(counts.bytes_current, 0);
}

#[test]
fn malformed_settings_and_blocks_are_refused_without_processing() {
    for (block_frames, lead_ms) in [(0, 50), (1025, 50), (usize::MAX, 50), (64, u32::MAX), (64, 1)] {
        assert!(matches!(
            Probe::new(renderer(&["utility"]), TrackId(1), Config { block_frames, lead_ms }),
            Err(ProbeError::InvalidSettings)
        ));
    }
    let mut probe = probe(64, 0, &["utility"]);
    assert_eq!(probe.set_path(Path::Ahead), Err(ProbeError::Disabled));
    let before = probe.status();
    for size in [0, 1, 127, 129, 130] {
        let mut out = vec![0.0; size];
        assert_eq!(probe.process(&mut out, true, &mut |_, _| panic!("must not render")), Err(ProbeError::BlockLength));
        assert_eq!(probe.status(), before);
    }
    probe.rendered = u64::MAX;
    assert_eq!(probe.process(&mut [0.0; 128], true, &mut signal), Err(ProbeError::PositionLimit));
}

#[test]
fn unsupported_tracks_are_not_silently_rendered_without_their_graph() {
    assert!(matches!(
        Probe::new(renderer(&["fm-synth"]), TrackId(1), Config { block_frames: 64, lead_ms: 50 }),
        Err(ProbeError::UnsupportedTrack)
    ));
    assert!(matches!(
        Probe::new(renderer(&["utility"]), TrackId(999), Config { block_frames: 64, lead_ms: 50 }),
        Err(ProbeError::UnsupportedTrack)
    ));
    let mut started = renderer(&["utility"]);
    started.process(&mut [0.0; 128]);
    assert!(matches!(
        Probe::new(started, TrackId(1), Config { block_frames: 64, lead_ms: 50 }),
        Err(ProbeError::AlreadyStarted)
    ));
}

#[test]
fn a_scheduled_source_impulse_keeps_its_sample_across_path_changes() {
    let mut probe = probe(31, 50, &["utility"]);
    let mut heard = Vec::new();
    let mut out = [0.0; 62];
    for block in 0..80 {
        if block == 0 || block == 35 {
            probe.set_path(Path::Ahead).unwrap();
        }
        if block == 10 || block == 55 {
            probe.set_path(Path::Live).unwrap();
        }
        probe
            .process(&mut out, true, &mut |at, audio| {
                audio.fill(0.0);
                for (offset, pair) in audio.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                    if [397, 701, 1597].contains(&(at + offset as u64)) {
                        pair.fill(0.25);
                    }
                }
            })
            .unwrap();
        heard.extend(
            out.as_chunks::<2>()
                .0
                .iter()
                .enumerate()
                .filter_map(|(i, pair)| (pair[0] != 0.0).then_some(block * 31 + i)),
        );
    }
    // Source scheduling only: no claim that this exercises the launcher's command timing.
    assert_eq!(heard, [397, 701, 1597]);
}
