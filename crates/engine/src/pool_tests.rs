// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn the_variable_asks_for_one_thread_unless_it_names_more() {
    assert_eq!(render_threads_from(None), 1);
    assert_eq!(render_threads_from(Some("0")), 1);
    assert_eq!(render_threads_from(Some("1")), 1);
    assert_eq!(render_threads_from(Some(" 4 ")), 4);
    assert_eq!(render_threads_from(Some("many")), 1);
    assert_eq!(render_threads_from(Some("1000")), MAX_RENDER_THREADS);
}

#[test]
fn a_pool_of_none_is_no_pool() {
    assert!(Pool::new(0, 8_000).is_none());
    let pool = Pool::new(3, 8_000);
    assert_eq!(pool.as_ref().map(Pool::threads), Some(4));
    assert!(!pool.as_ref().is_some_and(Pool::any_away));
}

use suhina_model::{ChannelId, ChannelRole, Clip, ContentId, Device, DeviceKind, DeviceParam, Note, Send};
use suhina_test_support::ProjectBuilder;

use crate::Renderer;

const SR: u32 = 48_000;
const BLOCK: usize = 512;

fn device(id: &str, params: &[(&str, f64)]) -> Device {
    Device {
        params: params.iter().map(|(p, v)| DeviceParam::new(*p, *v)).collect(),
        ..Device::new(id, DeviceKind::Builtin(id.into()))
    }
}

/// Four instrument tracks with effects: tracks 1 and 2 play into the main channel and send to
/// return 6 (a reverb), tracks 3 and 4 into group 5 (an EQ and a compressor), which plays into
/// the main channel too. Every chain may run on a worker.
fn project() -> suhina_model::Project {
    let notes = |key: u8| (0..8).map(|n| Note::new(f64::from(n) * 0.5, 0.4, key + (n % 3) as u8, 0.8)).collect();
    let mut builder = ProjectBuilder::new()
        .bus_track(5, "Group", ChannelRole::Submix)
        .bus_track(6, "Return", ChannelRole::Effect);
    let chains: [&[Device]; 4] = [
        &[device("fm-synth", &[]), device("overdrive", &[]), device("delay", &[])],
        &[device("fm-synth", &[]), device("saturator", &[]), device("auto-pan", &[])],
        &[device("fm-synth", &[]), device("compressor", &[]), device("reverb", &[])],
        &[device("fm-synth", &[]), device("eq", &[]), device("delay", &[])],
    ];
    for (n, devices) in (1u32..).zip(chains) {
        builder = builder
            .midi_track(n, &format!("T{n}"))
            .notes(n, &format!("c{n}"), 4.0, notes(48 + 7 * n as u8))
            .clip(n, Clip::new(ContentId(n), 0.0, 4.0));
        let devices = devices.to_vec();
        builder = builder.with(move |p| {
            if let Some(channel) = p.channel_mut(ChannelId(n)) {
                channel.devices = devices;
            }
        });
    }
    builder
        .output(3, 5)
        .parent(3, 5)
        .output(4, 5)
        .parent(4, 5)
        .with(|p| {
            for (source, level) in [(1, 0.5), (2, 0.7)] {
                if let Some(channel) = p.channel_mut(ChannelId(source)) {
                    channel.sends.push(Send { target: ChannelId(6), level, pre_fader: false });
                }
            }
            if let Some(group) = p.channel_mut(ChannelId(5)) {
                group.devices = vec![device("eq", &[]), device("compressor", &[])];
            }
            if let Some(ret) = p.channel_mut(ChannelId(6)) {
                ret.devices = vec![device("reverb", &[])];
            }
        })
        .build()
}

/// The project's first `blocks` blocks rendered on `threads` threads, and the chains the render
/// sent to workers.
fn render(threads: usize, blocks: usize) -> (Vec<f32>, u64) {
    let mut renderer = Renderer::new(&project(), SR, suhina_device_registry::factory()).unwrap();
    renderer.set_offline(true);
    renderer.set_render_threads(threads);
    assert_eq!(renderer.render_threads(), threads);
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut block = vec![0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    let sent = renderer.pool.as_ref().map_or(0, |pool| pool.sent);
    (out, sent)
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|s| s.to_bits()).collect()
}

#[test]
fn a_render_on_several_threads_is_the_render_on_one_bit_for_bit() {
    // Two seconds: the notes, the delays' repeats and the reverbs' tails.
    let blocks = 2 * SR as usize / BLOCK;
    let (alone, sent) = render(1, blocks);
    assert_eq!(sent, 0);
    assert!(suhina_test_support::peak(&alone) > 0.01, "the project plays");
    for threads in [2, 3, 4, 8] {
        let (parallel, sent) = render(threads, blocks);
        // Every block's first chains go to idle workers before the render thread runs one.
        assert!(sent >= blocks as u64, "{threads} threads sent {sent} chains to workers");
        assert!(bits(&parallel) == bits(&alone), "{threads} threads render other samples than one");
    }
}

#[test]
fn render_threads_can_be_changed_and_turned_off() {
    let mut renderer = Renderer::new(&project(), SR, suhina_device_registry::factory()).unwrap();
    assert_eq!(renderer.render_threads(), render_threads_from_env());
    renderer.set_render_threads(3);
    assert_eq!(renderer.render_threads(), 3);
    renderer.set_render_threads(0);
    assert_eq!(renderer.render_threads(), 1);
    renderer.set_render_threads(usize::MAX);
    assert_eq!(renderer.render_threads(), MAX_RENDER_THREADS);
}
