// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A render allocates and frees nothing on the render thread with the render workers on
//! (AGENTS.md rule 3, ADR 0038 › 2). `allocation-counter` counts what the measuring thread
//! allocates, reallocates and frees: on 4 threads that is the render thread's part (handing
//! chains out, taking them back, waiting); on 1 thread it is every chain's code, the code the
//! workers run.

#![allow(clippy::unwrap_used)] // a test's helper outside `#[test]` functions

use suhina_engine::Renderer;
use suhina_model::{ChannelId, ChannelRole, Clip, ContentId, Device, DeviceKind, Note, Send};
use suhina_test_support::ProjectBuilder;

const SR: u32 = 48_000;
const BLOCK: usize = 512;

fn device(id: &str) -> Device {
    Device::new(id, DeviceKind::Builtin(id.into()))
}

/// Six instrument tracks with effects, two into a group and two sending to a return: enough
/// chains for the render workers to take some every block.
fn project() -> suhina_model::Project {
    let mut builder =
        ProjectBuilder::new().bus_track(7, "Group", ChannelRole::Submix).bus_track(8, "Return", ChannelRole::Effect);
    for n in 1u32..=6 {
        let key = 40 + 5 * n as u8;
        let notes = (0..16).map(|i| Note::new(f64::from(i) * 0.25, 0.2, key + (i % 4) as u8, 0.8)).collect();
        builder = builder
            .midi_track(n, &format!("T{n}"))
            .notes(n, &format!("c{n}"), 4.0, notes)
            .clip(n, Clip::new(ContentId(n), 0.0, 4.0))
            .with(move |p| {
                if let Some(channel) = p.channel_mut(ChannelId(n)) {
                    channel.devices = vec![device("fm-synth"), device("saturator"), device("delay")];
                }
            });
    }
    builder
        .output(5, 7)
        .parent(5, 7)
        .output(6, 7)
        .parent(6, 7)
        .with(|p| {
            for source in [1, 2] {
                if let Some(channel) = p.channel_mut(ChannelId(source)) {
                    channel.sends.push(Send { target: ChannelId(8), level: 0.5, pre_fader: false });
                }
            }
            if let Some(group) = p.channel_mut(ChannelId(7)) {
                group.devices = vec![device("eq"), device("compressor")];
            }
            if let Some(ret) = p.channel_mut(ChannelId(8)) {
                ret.devices = vec![device("reverb")];
            }
        })
        .build()
}

/// What the render thread allocates over two seconds of the project on `threads` threads,
/// after the first blocks, which start the voices.
fn allocations(threads: usize) -> u64 {
    let mut renderer = Renderer::new(&project(), SR, suhina_device_registry::factory()).unwrap();
    renderer.set_offline(true);
    renderer.set_render_threads(threads);
    assert_eq!(renderer.render_threads(), threads);
    let mut block = vec![0.0f32; BLOCK * 2];
    for _ in 0..16 {
        renderer.process(&mut block);
    }
    let mut peak = 0.0f32;
    let counted = allocation_counter::measure(|| {
        for _ in 0..(2 * SR as usize / BLOCK) {
            renderer.process(&mut block);
            peak = block.iter().fold(peak, |p, s| p.max(s.abs()));
        }
    });
    assert!(peak > 0.01, "the project plays");
    counted.count_total
}

#[test]
fn a_render_allocates_nothing_on_one_thread_or_four() {
    assert_eq!(allocations(1), 0, "allocations rendering on 1 thread");
    assert_eq!(allocations(4), 0, "allocations on the render thread rendering on 4 threads");
}
